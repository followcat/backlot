//! Additional per-runner read-only transcript extractors (`cmd`, `cline`,
//! `agy`, `cursor`, `devin`, `opencode`).

use std::fs;
use std::io::{BufRead, BufReader};
use std::path::Path;
use std::time::{Duration, Instant};

use serde_json::Value;

use crate::discover::{Exec, ExecCall};
use crate::transcript::{MAX_TURNS, TranscriptTarget, Turn};
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
    let db = [
        home.join(".local/share/devin/sessions.db"),
        home.join(".devin/sessions.db"),
    ]
    .into_iter()
    .find(|p| p.is_file())?;
    let timeout = remaining(deadline)?;
    if !is_safe_id(sid) {
        return None;
    }
    let query = format!(
        "SELECT role, content FROM messages WHERE session_id = '{sid}' ORDER BY rowid DESC LIMIT {MAX_TURNS};"
    );
    let call = ExecCall {
        argv: vec![
            "sqlite3".into(),
            "-readonly".into(),
            "-json".into(),
            db.to_string_lossy().into_owned(),
            query,
        ],
        cwd: cwd.to_string(),
        timeout,
    };
    let out = exec(&call).ok()?;
    let mut arr: Vec<Value> = serde_json::from_str(&out).ok()?;
    arr.reverse();
    let turns = turns_from_message_array(&arr);
    (!turns.is_empty()).then(|| (format!("devin sessions.db ({})", db.display()), turns))
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
