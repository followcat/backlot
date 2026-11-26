//! Per-runner read-only transcript readers (`claude`, `codex`) plus shared
//! JSON content-block extraction for [`crate::transcript`].

use std::fs;
use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};
use std::time::Instant;

use serde_json::Value;

use crate::discover::Exec;
use crate::transcript::{MAX_TURNS, TranscriptTarget, Turn, collapse_tool};

pub(crate) fn read_turns(
    target: &TranscriptTarget<'_>,
    sid: &str,
    exec: &Exec,
    deadline: Instant,
) -> Option<(String, Vec<Turn>)> {
    let home = target.home?;
    match target.runner {
        "claude" => read_claude(home, target.cwd, sid, deadline),
        "codex" => read_codex(home, sid, deadline),
        _ => crate::transcript_ext::read_other_turns(target, home, sid, exec, deadline),
    }
}

fn read_claude(
    home: &Path,
    cwd: &str,
    sid: &str,
    deadline: Instant,
) -> Option<(String, Vec<Turn>)> {
    let projects = home.join(".claude").join("projects");
    let direct_slug = cwd.replace(['/', '.'], "-");
    let alt_slug = cwd.replace('/', "-");
    let mut path = [direct_slug, alt_slug]
        .into_iter()
        .map(|s| projects.join(s).join(format!("{sid}.jsonl")))
        .find(|p| p.is_file());
    if path.is_none()
        && let Ok(entries) = fs::read_dir(&projects)
    {
        for entry in entries.filter_map(|e| e.ok()).take(512) {
            if Instant::now() > deadline {
                return None;
            }
            let cand = entry.path().join(format!("{sid}.jsonl"));
            if cand.is_file() {
                path = Some(cand);
                break;
            }
        }
    }
    let path = path?;
    let turns = parse_claude_jsonl(&path, deadline)?;
    Some((format!("claude session {sid} ({})", path.display()), turns))
}

fn parse_claude_jsonl(path: &Path, deadline: Instant) -> Option<Vec<Turn>> {
    let file = fs::File::open(path).ok()?;
    let mut turns = Vec::new();
    for line in BufReader::new(file).lines().map_while(Result::ok) {
        if Instant::now() > deadline {
            break;
        }
        let Ok(v) = serde_json::from_str::<Value>(&line) else {
            continue;
        };
        let role = v["message"]["role"].as_str().or_else(|| v["type"].as_str());
        let Some(who) = role.filter(|r| matches!(*r, "user" | "assistant")) else {
            continue;
        };
        let content = v
            .get("message")
            .and_then(|m| m.get("content"))
            .or_else(|| v.get("content"));
        if let Some(text) = extract_content_blocks(content) {
            push_turn(&mut turns, who, text);
        }
    }
    (!turns.is_empty()).then_some(turns)
}

fn read_codex(home: &Path, sid: &str, deadline: Instant) -> Option<(String, Vec<Turn>)> {
    let path = find_codex_rollout(&home.join(".codex").join("sessions"), sid, deadline)?;
    let file = fs::File::open(&path).ok()?;
    let mut turns = Vec::new();
    for line in BufReader::new(file).lines().skip(1).map_while(Result::ok) {
        if Instant::now() > deadline {
            break;
        }
        let Ok(v) = serde_json::from_str::<Value>(&line) else {
            continue;
        };
        match v["type"].as_str() {
            Some("response_item") => {
                let p = &v["payload"];
                match p["type"].as_str() {
                    Some("message") => {
                        if let Some(who) = p["role"]
                            .as_str()
                            .filter(|r| matches!(*r, "user" | "assistant"))
                            && let Some(text) = extract_content_blocks(p.get("content"))
                        {
                            push_turn(&mut turns, who, text);
                        }
                    }
                    Some("function_call") => {
                        let name = p["name"].as_str().unwrap_or("call");
                        let args = p["arguments"].as_str().unwrap_or("");
                        push_turn(&mut turns, "assistant", collapse_tool(name, args));
                    }
                    Some("function_call_output") => {
                        let out = p["output"].as_str().unwrap_or("");
                        push_turn(&mut turns, "assistant", collapse_tool("result", out));
                    }
                    _ => {}
                }
            }
            Some("event_msg") => {
                let p = &v["payload"];
                match p["type"].as_str() {
                    Some("user_message") => {
                        if let Some(msg) = p["message"].as_str().or_else(|| p["text"].as_str()) {
                            push_turn(&mut turns, "user", msg.to_string());
                        }
                    }
                    Some("agent_message") => {
                        if let Some(msg) = p["message"].as_str().or_else(|| p["text"].as_str()) {
                            push_turn(&mut turns, "assistant", msg.to_string());
                        }
                    }
                    _ => {}
                }
            }
            _ => {
                if let Some(who) = v["role"]
                    .as_str()
                    .filter(|r| matches!(*r, "user" | "assistant"))
                    && let Some(text) = extract_content_blocks(v.get("content"))
                {
                    push_turn(&mut turns, who, text);
                }
            }
        }
    }
    (!turns.is_empty()).then(|| (format!("codex session {sid} ({})", path.display()), turns))
}

fn find_codex_rollout(root: &Path, sid: &str, deadline: Instant) -> Option<PathBuf> {
    let mut dirs = collect_recent_day_dirs(root, 60);
    dirs.sort();
    dirs.reverse();
    for day in dirs {
        if Instant::now() > deadline {
            return None;
        }
        let Ok(entries) = fs::read_dir(&day) else {
            continue;
        };
        let mut files: Vec<PathBuf> = entries
            .filter_map(|e| e.ok())
            .map(|e| e.path())
            .filter(|p| {
                p.file_name()
                    .and_then(|n| n.to_str())
                    .is_some_and(|n| n.starts_with("rollout-") && n.ends_with(".jsonl"))
            })
            .collect();
        files.sort();
        files.reverse();
        for f in files {
            if let Ok(file) = fs::File::open(&f) {
                let mut first = String::new();
                if BufReader::new(file).read_line(&mut first).is_ok()
                    && let Ok(v) = serde_json::from_str::<Value>(&first)
                    && v["payload"]["id"].as_str() == Some(sid)
                {
                    return Some(f);
                }
            }
        }
    }
    None
}

fn collect_recent_day_dirs(root: &Path, cap: usize) -> Vec<PathBuf> {
    let mut out = Vec::new();
    let Ok(years) = fs::read_dir(root) else {
        return out;
    };
    for y in years.filter_map(|e| e.ok()) {
        if !y.path().is_dir() {
            continue;
        }
        let Ok(months) = fs::read_dir(y.path()) else {
            continue;
        };
        for m in months.filter_map(|e| e.ok()) {
            if !m.path().is_dir() {
                continue;
            }
            let Ok(days) = fs::read_dir(m.path()) else {
                continue;
            };
            for d in days.filter_map(|e| e.ok()) {
                if d.path().is_dir() {
                    out.push(d.path());
                    if out.len() >= cap {
                        return out;
                    }
                }
            }
        }
    }
    out
}

pub(crate) fn turns_from_message_array(arr: &[Value]) -> Vec<Turn> {
    let mut turns = Vec::new();
    for v in arr {
        let Some(who) = v["role"]
            .as_str()
            .filter(|r| matches!(*r, "user" | "assistant"))
        else {
            continue;
        };
        if let Some(text) = extract_content_blocks(v.get("content")) {
            push_turn(&mut turns, who, text);
        }
    }
    turns
}

pub(crate) fn extract_content_blocks(content: Option<&Value>) -> Option<String> {
    let content = content?;
    match content {
        Value::String(s) => (!s.trim().is_empty()).then(|| s.trim().to_string()),
        Value::Array(items) => {
            let mut parts = Vec::new();
            for item in items {
                match item["type"].as_str() {
                    Some("text" | "input_text" | "output_text") => {
                        if let Some(t) = item["text"].as_str().filter(|t| !t.trim().is_empty()) {
                            parts.push(t.trim().to_string());
                        }
                    }
                    Some("tool_use" | "tool-invocation") => {
                        let name = item["name"]
                            .as_str()
                            .or_else(|| item["toolName"].as_str())
                            .unwrap_or("tool");
                        let arg = item.get("input").map(|v| v.to_string()).unwrap_or_default();
                        parts.push(collapse_tool(name, &arg));
                    }
                    Some("tool_result") => {
                        let out = item
                            .get("content")
                            .and_then(|c| c.as_str())
                            .unwrap_or("done");
                        parts.push(collapse_tool("result", out));
                    }
                    _ => {
                        if let Some(t) = item["text"].as_str().filter(|t| !t.trim().is_empty()) {
                            parts.push(t.trim().to_string());
                        }
                    }
                }
            }
            (!parts.is_empty()).then(|| parts.join("\n"))
        }
        Value::Object(map) => map
            .get("text")
            .and_then(|v| v.as_str())
            .filter(|s| !s.trim().is_empty())
            .map(|s| s.trim().to_string()),
        _ => None,
    }
}

pub(crate) fn push_turn(turns: &mut Vec<Turn>, who: &str, text: String) {
    turns.push(Turn {
        who: who.to_string(),
        text,
    });
    if turns.len() > MAX_TURNS * 2 {
        let excess = turns.len() - MAX_TURNS * 2;
        turns.drain(..excess);
    }
}
