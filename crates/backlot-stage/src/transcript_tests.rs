//! Fail-first tests for runner-generic conversation tail extraction,
//! secret redaction, tool call collapsing, and universal scrollback fallback.

use std::fs;
use std::path::Path;

use crate::discover::ExecCall;
use crate::test_support::TestDir;
use crate::transcript::{
    EXTRACT_TIMEOUT, MAX_TAIL_BYTES, TailKind, TranscriptTarget, extract_runner_tail,
    format_fallback_tail, redact_secrets,
};

fn no_exec(_call: &ExecCall) -> Result<String, String> {
    Err("no exec".into())
}

#[test]
fn redacts_tokens_keys_env_assignments_and_urls() {
    let raw = concat!(
        "Authorization: Bearer tok_super_secret_123456\n",
        "key=sk-proj-abcdef0123456789\n",
        "gh=ghp_1234567890abcdef1234\n",
        "gcp=AIzaSyDummyKeyValue123456\n",
        "MY_API_TOKEN=secret_val_999 AWS_SECRET_KEY=\"quoted-secret\"\n",
        "proxy=http://alice:hunter2@proxy.example:8080/path\n",
        "safe_line: working on acme-shop\n",
    );
    let clean = redact_secrets(raw);
    assert!(!clean.contains("tok_super_secret_123456"), "{clean}");
    assert!(!clean.contains("abcdef0123456789"), "{clean}");
    assert!(!clean.contains("1234567890abcdef1234"), "{clean}");
    assert!(!clean.contains("DummyKeyValue123456"), "{clean}");
    assert!(!clean.contains("secret_val_999"), "{clean}");
    assert!(!clean.contains("quoted-secret"), "{clean}");
    assert!(!clean.contains("hunter2"), "{clean}");
    assert!(clean.contains("Bearer [REDACTED]"), "{clean}");
    assert!(clean.contains("MY_API_TOKEN=[REDACTED]"), "{clean}");
    assert!(clean.contains("AWS_SECRET_KEY=[REDACTED]"), "{clean}");
    assert!(clean.contains("safe_line: working on acme-shop"), "{clean}");
}

#[test]
fn extracts_claude_codex_cmd_cline_and_agy_tails_and_collapses_tools() {
    let tmp = TestDir::new("bl-transcript");
    let home = tmp.path().join("home");
    let role_dir = tmp.path().join("hollywood/roles/hollywood");
    fs::create_dir_all(&role_dir).unwrap();
    let cwd = "/home/user/hollywood";

    // 1. Claude JSONL with tool_use > 300 chars and secret token
    let claude_proj = home.join(".claude/projects/-home-user-hollywood");
    fs::create_dir_all(&claude_proj).unwrap();
    let long_cmd = "x".repeat(400);
    let claude_lines = [
        serde_json::json!({"type": "user", "message": {"role": "user", "content": "check acme-shop"}}).to_string(),
        serde_json::json!({
            "type": "assistant",
            "message": {
                "role": "assistant",
                "content": [
                    {"type": "tool_use", "name": "Bash", "input": {"command": long_cmd}},
                    {"type": "text", "text": "Acme shop looks idle. Token sk-live-secret999999."}
                ]
            }
        }).to_string(),
    ];
    fs::write(
        claude_proj.join("sid-claude.jsonl"),
        claude_lines.join("\n") + "\n",
    )
    .unwrap();

    let claude_tail = extract_runner_tail(
        &TranscriptTarget {
            cwd,
            runner: "claude",
            session_id: Some("sid-claude"),
            role_dir: &role_dir,
            home: Some(&home),
        },
        &no_exec,
    )
    .expect("claude tail");
    assert_eq!(claude_tail.kind, TailKind::RunnerSession);
    assert!(claude_tail.markdown.contains("check acme-shop"));
    assert!(claude_tail.markdown.contains("[tool: Bash]"));
    assert!(!claude_tail.markdown.contains(&"x".repeat(350)));
    assert!(!claude_tail.markdown.contains("secret999999"));

    // 2. Codex rollout JSONL
    let codex_day = home.join(".codex/sessions/2026/11/26");
    fs::create_dir_all(&codex_day).unwrap();
    let codex_lines = [
        serde_json::json!({"type": "session_meta", "payload": {"id": "sid-codex", "cwd": cwd}}).to_string(),
        serde_json::json!({
            "type": "response_item",
            "payload": {"type": "message", "role": "user", "content": [{"type": "input_text", "text": "pause t1"}]}
        }).to_string(),
        serde_json::json!({
            "type": "response_item",
            "payload": {"type": "message", "role": "assistant", "content": [{"type": "output_text", "text": "proposed pause card"}]}
        }).to_string(),
    ];
    fs::write(
        codex_day.join("rollout-2026-11-26-sid-codex.jsonl"),
        codex_lines.join("\n") + "\n",
    )
    .unwrap();
    let codex_tail = extract_runner_tail(
        &TranscriptTarget {
            cwd,
            runner: "codex",
            session_id: Some("sid-codex"),
            role_dir: &role_dir,
            home: Some(&home),
        },
        &no_exec,
    )
    .expect("codex tail");
    assert!(codex_tail.markdown.contains("pause t1"));
    assert!(codex_tail.markdown.contains("proposed pause card"));

    // 3. cmd JSONL
    let cmd_dir = home
        .join(".commandcode/projects")
        .join(crate::runners::cmd::slug(cwd));
    fs::create_dir_all(&cmd_dir).unwrap();
    fs::write(
        cmd_dir.join("sid-cmd.jsonl"),
        format!(
            "{}\n{}\n",
            serde_json::json!({"type": "session", "id": "sid-cmd", "cwd": cwd}),
            serde_json::json!({"role": "assistant", "content": "cmd summary here"}),
        ),
    )
    .unwrap();
    let cmd_tail = extract_runner_tail(
        &TranscriptTarget {
            cwd,
            runner: "cmd",
            session_id: Some("sid-cmd"),
            role_dir: &role_dir,
            home: Some(&home),
        },
        &no_exec,
    )
    .expect("cmd tail");
    assert!(cmd_tail.markdown.contains("cmd summary here"));

    // 4. cline messages.json
    let cline_dir = home.join(".cline/data/sessions/sid-cline");
    fs::create_dir_all(&cline_dir).unwrap();
    fs::write(
        cline_dir.join("sid-cline.messages.json"),
        serde_json::json!([
            {"role": "user", "content": "cline user prompt"},
            {"role": "assistant", "content": [{"type": "text", "text": "cline reply"}]}
        ])
        .to_string(),
    )
    .unwrap();
    let cline_tail = extract_runner_tail(
        &TranscriptTarget {
            cwd,
            runner: "cline",
            session_id: Some("sid-cline"),
            role_dir: &role_dir,
            home: Some(&home),
        },
        &no_exec,
    )
    .expect("cline tail");
    assert!(cline_tail.markdown.contains("cline reply"));

    // 5. agy transcript.jsonl
    let agy_logs = home.join(".gemini/antigravity-cli/brain/sid-agy/.system_generated/logs");
    fs::create_dir_all(&agy_logs).unwrap();
    fs::write(
        agy_logs.join("transcript.jsonl"),
        format!(
            "{}\n",
            serde_json::json!({"role": "assistant", "text": "agy turn output"})
        ),
    )
    .unwrap();
    let agy_tail = extract_runner_tail(
        &TranscriptTarget {
            cwd,
            runner: "agy",
            session_id: Some("sid-agy"),
            role_dir: &role_dir,
            home: Some(&home),
        },
        &no_exec,
    )
    .expect("agy tail");
    assert!(agy_tail.markdown.contains("agy turn output"));
}

#[test]
fn extracts_opencode_via_exec_and_bounds_tail_to_16_kib() {
    let tmp = TestDir::new("bl-transcript-oc");
    let home = tmp.path().join("home");
    let role_dir = tmp.path().join("hollywood/roles/hollywood");
    fs::create_dir_all(&role_dir).unwrap();

    let exec_oc = |call: &ExecCall| -> Result<String, String> {
        assert!(
            call.timeout <= EXTRACT_TIMEOUT && call.timeout >= std::time::Duration::from_secs(2)
        );
        assert_eq!(
            call.argv,
            vec!["opencode", "session", "export", "ses_123", "--sanitize"]
        );
        Ok(serde_json::json!({
            "messages": [
                {"info": {"role": "user"}, "parts": [{"type": "text", "text": "older turn"}]},
                {"info": {"role": "assistant"}, "parts": [{"type": "text", "text": "newest answer"}]}
            ]
        })
        .to_string())
    };
    let tail = extract_runner_tail(
        &TranscriptTarget {
            cwd: "/home/user/hollywood",
            runner: "opencode",
            session_id: Some("ses_123"),
            role_dir: &role_dir,
            home: Some(&home),
        },
        &exec_oc,
    )
    .expect("opencode tail");
    assert!(tail.markdown.contains("newest answer"));

    // Oversized tail trims oldest turns to <= 16 KiB while keeping newest.
    let claude_proj = home.join(".claude/projects/-home-user-hollywood");
    fs::create_dir_all(&claude_proj).unwrap();
    let mut big = String::new();
    for i in 0..40 {
        let line = serde_json::json!({
            "type": "assistant",
            "message": {"role": "assistant", "content": format!("turn-{i:02} {}", "w".repeat(700))}
        })
        .to_string();
        big.push_str(&line);
        big.push('\n');
    }
    fs::write(claude_proj.join("sid-big.jsonl"), big).unwrap();
    let big_tail = extract_runner_tail(
        &TranscriptTarget {
            cwd: "/home/user/hollywood",
            runner: "claude",
            session_id: Some("sid-big"),
            role_dir: &role_dir,
            home: Some(&home),
        },
        &no_exec,
    )
    .expect("big tail");
    assert!(big_tail.markdown.len() <= MAX_TAIL_BYTES);
    assert!(big_tail.markdown.contains("turn-39"));
    assert!(!big_tail.markdown.contains("turn-00"));
}

#[test]
fn universal_fallback_strips_ansi_redacts_secrets_and_reports_provenance() {
    let scrollback = "\x1b[32m> status ok\x1b[0m\nBearer topsecret999999\n";
    let ledger = vec![
        "seq 4 [user -> hollywood] pause beta-api".to_string(),
        "seq 5 [hollywood] reported: all clear".to_string(),
    ];
    let fb = format_fallback_tail(Some(scrollback), &ledger);
    assert_eq!(fb.kind, TailKind::TerminalFallback);
    assert!(
        fb.provenance
            .contains("terminal scrollback + ledger exchange")
    );
    assert!(!fb.markdown.contains("\x1b["));
    assert!(!fb.markdown.contains("topsecret999999"));
    assert!(fb.markdown.contains("Bearer [REDACTED]"));
    assert!(fb.markdown.contains("pause beta-api"));
    assert!(Path::new("x").extension().is_none());
}

#[test]
fn extracts_devin_cli_sqlite_schema_follows_parent_chain_and_skips_thinking_images() {
    let tmp = TestDir::new("bl-transcript-devin");
    let home = tmp.path().join("home");
    let role_dir = tmp.path().join("hollywood/roles/hollywood");
    let db_dir = home.join(".local/share/devin/cli");
    fs::create_dir_all(&role_dir).unwrap();
    fs::create_dir_all(&db_dir).unwrap();
    let db_path = db_dir.join("sessions.db");

    let sys_msg = serde_json::json!({
        "role": "system",
        "content": "INTERNAL_SYSTEM_PROMPT_SKIP"
    })
    .to_string();
    let u1 = serde_json::json!({
        "role": "user",
        "content": "Check acme-shop failing tests."
    })
    .to_string();
    let branch_dead = serde_json::json!({
        "role": "assistant",
        "content": "DEAD_BRANCH_MESSAGE_SKIP"
    })
    .to_string();
    let a1 = serde_json::json!({
        "role": "assistant",
        "thinking": "SECRET_THINKING_DO_NOT_COPY",
        "content": "Inspecting cargo output.",
        "images": [{"media_type": "image/png", "base64_data": "BASE64_IMAGE_PAYLOAD_SKIP"}],
        "tool_calls": [{"id": "tc-1", "name": "read_file", "arguments": "{\"path\":\"Cargo.toml\"}"}]
    })
    .to_string();
    let t1 = serde_json::json!({
        "role": "tool",
        "tool_call_id": "tc-1",
        "content": "3 failed in auth module"
    })
    .to_string();
    let a2 = serde_json::json!({
        "role": "assistant",
        "content": "Proposed pause card for acme-shop."
    })
    .to_string();

    // Populate SQLite fixture using python3 built-in sqlite3 (works with or without sqlite3 CLI).
    let py = r#"
import sqlite3, sys
db = sys.argv[1]
con = sqlite3.connect(db)
con.execute("""CREATE TABLE sessions(
    id TEXT PRIMARY KEY,
    working_directory TEXT,
    backend_type TEXT,
    model TEXT,
    agent_mode TEXT,
    created_at INTEGER,
    last_activity_at INTEGER
)""")
con.execute("""CREATE TABLE message_nodes(
    row_id INTEGER PRIMARY KEY AUTOINCREMENT,
    session_id TEXT,
    node_id INTEGER,
    parent_node_id INTEGER NULL,
    chat_message TEXT,
    created_at INTEGER,
    metadata TEXT,
    UNIQUE(session_id, node_id)
)""")
con.execute("CREATE INDEX idx_message_nodes_session ON message_nodes(session_id)")
con.execute("INSERT INTO sessions VALUES('sid_devin_1', '/home/user/hollywood', 'cli', 'swe-2-max', 'default', 1, 2)")
rows = [
    ('sid_devin_1', 1, None, sys.argv[2], 1, '{}'),
    ('sid_devin_1', 2, 1, sys.argv[3], 2, '{}'),
    ('sid_devin_1', 3, 2, sys.argv[4], 3, '{}'),  # orphaned branch
    ('sid_devin_1', 4, 2, sys.argv[5], 4, '{}'),  # active chain: a1
    ('sid_devin_1', 5, 4, sys.argv[6], 5, '{}'),  # active chain: t1
    ('sid_devin_1', 6, 5, sys.argv[7], 6, '{}'),  # active chain: a2
]
con.executemany("INSERT INTO message_nodes(session_id, node_id, parent_node_id, chat_message, created_at, metadata) VALUES(?,?,?,?,?,?)", rows)
con.commit()
"#;
    let status = std::process::Command::new("python3")
        .arg("-c")
        .arg(py)
        .arg(&db_path)
        .arg(&sys_msg)
        .arg(&u1)
        .arg(&branch_dead)
        .arg(&a1)
        .arg(&t1)
        .arg(&a2)
        .status()
        .expect("python3 sqlite3 fixture");
    assert!(status.success());

    // First verify real_exec path extracts from real cli/sessions.db.
    let tail = extract_runner_tail(
        &TranscriptTarget {
            cwd: "/home/user/hollywood",
            runner: "devin",
            session_id: Some("sid_devin_1"),
            role_dir: &role_dir,
            home: Some(&home),
        },
        &crate::discover::real_exec,
    )
    .expect("devin tail");
    assert!(tail.provenance.contains("cli/sessions.db"), "{tail:?}");
    assert!(tail.markdown.contains("Check acme-shop failing tests."));
    assert!(tail.markdown.contains("Inspecting cargo output."));
    assert!(tail.markdown.contains("[tool: read_file]"));
    assert!(tail.markdown.contains("Proposed pause card for acme-shop."));
    assert!(tail.markdown.contains("3 failed in auth module"));
    assert!(!tail.markdown.contains("INTERNAL_SYSTEM_PROMPT_SKIP"));
    assert!(!tail.markdown.contains("DEAD_BRANCH_MESSAGE_SKIP"));
    assert!(!tail.markdown.contains("SECRET_THINKING_DO_NOT_COPY"));
    assert!(!tail.markdown.contains("BASE64_IMAGE_PAYLOAD_SKIP"));

    // Also verify the exact sqlite3 -readonly -json indexed query issued to Exec.
    let seen_call = std::sync::Arc::new(std::sync::Mutex::new(None));
    let seen_clone = seen_call.clone();
    let u1_copy = u1.clone();
    let a2_copy = a2.clone();
    let spy_exec = move |call: &ExecCall| -> Result<String, String> {
        *seen_clone.lock().unwrap() = Some(call.argv.clone());
        Ok(serde_json::json!([
            {"node_id": 2, "parent_node_id": 1, "chat_message": a2_copy},
            {"node_id": 1, "parent_node_id": null, "chat_message": u1_copy}
        ])
        .to_string())
    };
    let spy_tail = extract_runner_tail(
        &TranscriptTarget {
            cwd: "/home/user/hollywood",
            runner: "devin",
            session_id: Some("sid_devin_1"),
            role_dir: &role_dir,
            home: Some(&home),
        },
        &spy_exec,
    )
    .expect("spy devin tail");
    assert!(spy_tail.markdown.contains("Proposed pause card"));
    let argv = seen_call.lock().unwrap().clone().unwrap();
    assert_eq!(argv[0], "sqlite3");
    assert_eq!(argv[1], "-readonly");
    assert_eq!(argv[2], "-json");
    assert!(argv[3].ends_with(".local/share/devin/cli/sessions.db"));
    assert!(argv[4].contains("WHERE session_id = 'sid_devin_1'"));
    assert!(argv[4].contains("ORDER BY node_id DESC"));
}
