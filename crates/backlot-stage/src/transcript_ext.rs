//! Additional per-runner read-only transcript extractors (`cmd`, `cline`,
//! `agy`, `cursor`, `devin`, `opencode`).

use std::collections::{HashMap, HashSet};
use std::fs;
use std::io::{BufRead, BufReader};
use std::path::Path;
use std::time::{Duration, Instant};

use serde_json::Value;

use crate::discover::{Exec, ExecCall};
use crate::transcript::{MAX_TURNS, TranscriptTarget, Turn, collapse_tool};
use crate::transcript_runners::{extract_content_blocks, push_turn, turns_from_message_array};

pub(crate) fn read_other_turns(
    target: &TranscriptTarget<'_>,
    home: &Path,
    sid: &str,
    exec: &Exec,
    deadline: Instant,
) -> Option<(String, Vec<Turn>)> {
    match target.runner {
        "cmd" => read_cmd(home, target.cwd, sid),
        "cline" => read_cline(home, sid),
        "agy" => read_agy(home, sid),
        "cursor" => read_cursor(target.role_dir, sid),
        "devin" => read_devin(home, target.cwd, sid, exec, deadline),
        "opencode" => read_opencode(target.cwd, sid, exec, deadline),
        _ => None,
    }
}

fn read_cmd(home: &Path, cwd: &str, sid: &str) -> Option<(String, Vec<Turn>)> {
    let path = home
        .join(".commandcode/projects")
        .join(crate::runners::cmd::slug(cwd))
        .join(format!("{sid}.jsonl"));
    let file = fs::File::open(&path).ok()?;
    let mut turns = Vec::new();
    for line in BufReader::new(file).lines().skip(1).map_while(Result::ok) {
        let Ok(v) = serde_json::from_str::<Value>(&line) else {
            continue;
        };
        let who = v["role"].as_str().or_else(|| v["type"].as_str());
        if let Some(who) = who.filter(|r| matches!(*r, "user" | "assistant")) {
            let content = v
                .get("content")
                .or_else(|| v.get("message").and_then(|m| m.get("content")))
                .or_else(|| v.get("text"));
            if let Some(text) = extract_content_blocks(content) {
                push_turn(&mut turns, who, text);
            }
        }
    }
    (!turns.is_empty()).then(|| (format!("cmd session {sid} ({})", path.display()), turns))
}

fn read_cline(home: &Path, sid: &str) -> Option<(String, Vec<Turn>)> {
    let path = crate::runners::cline::sessions_root(home)
        .join(sid)
        .join(format!("{sid}.messages.json"));
    let text = fs::read_to_string(&path).ok()?;
    let arr: Vec<Value> = serde_json::from_str(&text).ok()?;
    let turns = turns_from_message_array(&arr);
    (!turns.is_empty()).then(|| (format!("cline session {sid} ({})", path.display()), turns))
}

fn read_agy(home: &Path, sid: &str) -> Option<(String, Vec<Turn>)> {
    let path = home
        .join(".gemini/antigravity-cli/brain")
        .join(sid)
        .join(".system_generated/logs/transcript.jsonl");
    let file = fs::File::open(&path).ok()?;
    let mut turns = Vec::new();
    for line in BufReader::new(file).lines().map_while(Result::ok) {
        let Ok(v) = serde_json::from_str::<Value>(&line) else {
            continue;
        };
        let who = v["role"]
            .as_str()
            .or_else(|| v["author"].as_str())
            .or_else(|| v["type"].as_str());
        if let Some(who) = who.filter(|r| matches!(*r, "user" | "assistant" | "model")) {
            let mapped = if who == "model" { "assistant" } else { who };
            let content = v
                .get("content")
                .or_else(|| v.get("text"))
                .or_else(|| v.get("message"));
            if let Some(text) = extract_content_blocks(content) {
                push_turn(&mut turns, mapped, text);
            }
        }
    }
    (!turns.is_empty()).then(|| (format!("agy transcript {sid} ({})", path.display()), turns))
}

fn read_cursor(role_dir: &Path, sid: &str) -> Option<(String, Vec<Turn>)> {
    let chats = crate::runners::cursor::chats_root(&role_dir.join("cursor"));
    let entries = fs::read_dir(chats).ok()?;
    for hash_dir in entries.filter_map(|e| e.ok()) {
        let dir = hash_dir.path().join(sid);
        if !dir.is_dir() {
            continue;
        }
        for fname in ["messages.json", "transcript.json"] {
            let p = dir.join(fname);
            if let Ok(text) = fs::read_to_string(&p)
                && let Ok(arr) = serde_json::from_str::<Vec<Value>>(&text)
            {
                let turns = turns_from_message_array(&arr);
                if !turns.is_empty() {
                    return Some((format!("cursor chat {sid} ({})", p.display()), turns));
                }
            }
        }
    }
    None
}

fn read_devin(
    home: &Path,
    cwd: &str,
    sid: &str,
    exec: &Exec,
    deadline: Instant,
) -> Option<(String, Vec<Turn>)> {
    let db = home.join(".local/share/devin/cli/sessions.db");
    if !db.is_file() || !is_safe_id(sid) {
        return None;
    }
    let db_str = db.to_string_lossy().into_owned();
    let exec_cwd = if Path::new(cwd).is_dir() {
        cwd.to_string()
    } else {
        db.parent()
            .map(|p| p.to_string_lossy().into_owned())
            .unwrap_or_else(|| ".".to_string())
    };
    let timeout = remaining(deadline)?;
    // Index-backed single-session lookup on message_nodes(session_id) — never scan the 8 GB DB.
    let limit = MAX_TURNS * 4;
    let query = format!(
        "SELECT node_id, parent_node_id, chat_message FROM message_nodes WHERE session_id = '{sid}' ORDER BY node_id DESC LIMIT {limit};"
    );
    let sqlite_call = ExecCall {
        argv: vec![
            "sqlite3".into(),
            "-readonly".into(),
            "-json".into(),
            db_str.clone(),
            query,
        ],
        cwd: exec_cwd.clone(),
        timeout,
    };
    let out = match exec(&sqlite_call) {
        Ok(s) => s,
        Err(_) => {
            // Headless hosts without `sqlite3` CLI still open the SQLite file read-only via python3.
            let py_timeout = remaining(deadline)?;
            let script = format!(
                "import json, sqlite3, sys, urllib.parse; \
                 uri = 'file:' + urllib.parse.quote(sys.argv[1], safe='/') + '?mode=ro'; \
                 con = sqlite3.connect(uri, uri=True, timeout=1.0); \
                 cur = con.execute('SELECT node_id, parent_node_id, chat_message FROM message_nodes WHERE session_id = ? ORDER BY node_id DESC LIMIT {limit}', (sys.argv[2],)); \
                 print(json.dumps([{{'node_id': r[0], 'parent_node_id': r[1], 'chat_message': r[2]}} for r in cur.fetchall()]))"
            );
            exec(&ExecCall {
                argv: vec!["python3".into(), "-c".into(), script, db_str, sid.into()],
                cwd: exec_cwd,
                timeout: py_timeout,
            })
            .ok()?
        }
    };
    if out.trim().is_empty() {
        return None;
    }
    let mut rows: Vec<Value> = serde_json::from_str(&out).ok()?;
    rows.sort_by_key(|r| std::cmp::Reverse(r["node_id"].as_i64().unwrap_or(0)));
    let chain = walk_devin_chain(&rows);
    let mut turns = Vec::new();
    for row in chain {
        if let Some((who, text)) = parse_devin_chat_message(&row["chat_message"]) {
            push_turn(&mut turns, who, text);
        }
    }
    (!turns.is_empty()).then(|| (format!("devin cli/sessions.db ({})", db.display()), turns))
}

fn walk_devin_chain(rows: &[Value]) -> Vec<&Value> {
    let by_id: HashMap<i64, &Value> = rows
        .iter()
        .filter_map(|r| r["node_id"].as_i64().map(|id| (id, r)))
        .collect();
    let Some(mut cur_id) = rows.first().and_then(|r| r["node_id"].as_i64()) else {
        return Vec::new();
    };
    let mut visited = HashSet::new();
    let mut chain = Vec::new();
    while visited.insert(cur_id)
        && let Some(node) = by_id.get(&cur_id).copied()
    {
        chain.push(node);
        let Some(parent) = node["parent_node_id"].as_i64() else {
            break;
        };
        cur_id = parent;
    }
    chain.reverse();
    chain
}

fn parse_devin_chat_message(raw: &Value) -> Option<(&'static str, String)> {
    let owned;
    let cm = match raw {
        Value::String(s) => {
            owned = serde_json::from_str::<Value>(s).ok()?;
            &owned
        }
        Value::Object(_) => raw,
        _ => return None,
    };
    let role = cm["role"].as_str()?;
    match role {
        "user" | "assistant" => {
            let who = if role == "user" { "user" } else { "assistant" };
            let mut parts = Vec::new();
            // Explicitly skip `thinking` and `images` (`base64_data` must never enter the brief).
            if let Some(text) = cm
                .get("content")
                .and_then(|c| c.as_str())
                .map(str::trim)
                .filter(|s| !s.is_empty())
            {
                parts.push(text.to_string());
            }
            if let Some(calls) = cm.get("tool_calls").and_then(|v| v.as_array()) {
                for call in calls {
                    let name = call["name"].as_str().unwrap_or("tool");
                    parts.push(format!("[tool: {name}]"));
                }
            }
            (!parts.is_empty()).then(|| (who, parts.join("\n")))
        }
        "tool" => {
            let text = cm
                .get("content")
                .and_then(|c| c.as_str())
                .map(str::trim)
                .filter(|s| !s.is_empty())?;
            Some(("assistant", collapse_tool("result", text)))
        }
        _ => None,
    }
}

fn read_opencode(
    cwd: &str,
    sid: &str,
    exec: &Exec,
    deadline: Instant,
) -> Option<(String, Vec<Turn>)> {
    if !is_safe_id(sid) {
        return None;
    }
    let timeout = remaining(deadline)?;
    let call = ExecCall {
        argv: vec![
            "opencode".into(),
            "session".into(),
            "export".into(),
            sid.to_string(),
            "--sanitize".into(),
        ],
        cwd: cwd.to_string(),
        timeout,
    };
    let out = exec(&call).ok()?;
    let v: Value = serde_json::from_str(&out).ok()?;
    let msgs = v
        .get("messages")
        .and_then(|m| m.as_array())
        .or_else(|| v.as_array())?;
    let mut turns = Vec::new();
    for m in msgs {
        let who = m["info"]["role"]
            .as_str()
            .or_else(|| m["role"].as_str())
            .filter(|r| matches!(*r, "user" | "assistant"));
        let Some(who) = who else {
            continue;
        };
        let parts = m.get("parts").or_else(|| m.get("content"));
        if let Some(text) = extract_content_blocks(parts) {
            push_turn(&mut turns, who, text);
        }
    }
    (!turns.is_empty()).then(|| (format!("opencode export {sid}"), turns))
}

fn is_safe_id(s: &str) -> bool {
    !s.is_empty()
        && s.bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_'))
}

fn remaining(deadline: Instant) -> Option<Duration> {
    let rem = deadline.saturating_duration_since(Instant::now());
    (!rem.is_zero()).then_some(rem)
}
