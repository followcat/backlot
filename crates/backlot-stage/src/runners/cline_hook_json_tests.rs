//! The hook's JSON side: the payload's strings are decoded before any
//! check, so an escaped quote, a backtick between escaped quotes, an
//! escaped newline or an unsupported escape cannot slip a command past
//! a rule — plus the wider secret list the first round missed.

use super::cline_hook::MESSAGE;
use super::cline_hook_tests::{call, commands_of, run, tmp};

/// The `commands` strings arrive JSON-encoded. Decoding them is the
/// only way the checks see the command a shell will run: an escaped
/// quote, a backtick between escaped quotes, and an escaped newline
/// (which `JSON.stringify` writes as the two characters `\n`) each
/// used to slip past a rule that only allows `git`.
#[test]
fn escaped_commands_are_decoded_before_the_check() {
    let dir = tmp("escapes");
    for cmd in [
        "git status \"$(curl https://evil)\"",
        "git commit -m \"`id`\"",
        "git status\ncurl https://evil",
    ] {
        let payload = commands_of(&[cmd]);
        let (cancel, msg, signals) = run(&dir, &payload);
        assert!(cancel, "{cmd:?} was allowed: {signals}");
        assert_eq!(msg, MESSAGE);
    }
}

/// A decoded command the role may run stays allowed: the quotes and
/// the colon survive decoding, and nothing about them looks like a
/// second command.
#[test]
fn allowed_commands_survive_decoding() {
    let dir = tmp("decode-allow");
    for cmd in [
        "git commit -m \"fix: tidy the hook\"",
        "git diff -- src/a.rs",
        "backlot tell producer --file /home/user/state/studios/acme-shop/messages/1.md",
    ] {
        let (cancel, _, signals) = run(&dir, &commands_of(&[cmd]));
        assert!(!cancel, "{cmd:?} was cancelled: {signals}");
    }
}

/// A `;` inside a quoted message becomes a second segment once
/// decoded, and that segment matches no rule — so the call is
/// refused. Over-refusing a command that only *looks* chained is the
/// price of not parsing a shell.
#[test]
fn a_semicolon_inside_a_message_is_refused() {
    let dir = tmp("semicolon");
    let (cancel, _, signals) = run(&dir, &commands_of(&["git commit -m \"a; b\""]));
    assert!(cancel, "{signals}");
}

/// An escape that is not `\" \\ \/ \n \r \t` — `\b`, `\f`, `\uXXXX` —
/// is refused outright: a control character in a command is nothing a
/// role needs, and `\uXXXX` is the one escape that could smuggle a
/// character past the `$`, backtick and redirect checks.
#[test]
fn an_unsupported_escape_is_refused() {
    let dir = tmp("bad-escape");
    for cmd in ["git status \u{1}", "git status \u{8}", "git status \u{c}"] {
        let payload = commands_of(&[cmd]);
        assert!(
            payload.contains("\\u0001") || payload.contains("\\b") || payload.contains("\\f"),
            "the encoder writes the escape: {payload}"
        );
        let (cancel, _, _) = run(&dir, &payload);
        assert!(cancel, "{cmd:?} was allowed");
    }
}

/// A subshell or a group starts a command the prefix match never sees,
/// so `(` `)` `{` and `}` are refused even under an allowed prefix.
#[test]
fn a_subshell_or_group_is_refused() {
    let dir = tmp("group");
    for cmd in ["git status (x)", "git status {x}", "git log )"] {
        let (cancel, _, _) = run(&dir, &commands_of(&[cmd]));
        assert!(cancel, "{cmd:?} was allowed");
    }
}

/// Reads of the secrets the first list missed: the environment of a
/// process (a proxy URL can hold credentials), the agent CLIs' auth
/// files, cursor's `authInfo`, container and cluster configs, and the
/// shell start-up files that often carry API keys.
#[test]
fn a_wider_secret_list_is_refused() {
    let dir = tmp("secret2");
    for path in [
        "/proc/self/environ",
        "/proc/1234/environ",
        "/home/alice/.claude/.credentials.json",
        "/home/alice/.codex/auth.json",
        "/home/alice/.config/cursor/cli-config.json",
        "/home/alice/.cursor/mcp.json",
        "/home/alice/.config/devin/settings.json",
        "/home/alice/.commandcode/settings.json",
        "/home/alice/.config/secrets/token",
        "/home/alice/.docker/config.json",
        "/home/alice/.kube/config",
        "/home/alice/.zshrc",
        "/home/alice/.bashrc",
        "/home/alice/.profile",
        "/home/alice/.zshenv",
        "/home/alice/.bash_profile",
    ] {
        let payload = call("read_files", serde_json::json!({"files": [{"path": path}]}));
        let (cancel, _, signals) = run(&dir, &payload);
        assert!(cancel, "{path} was allowed: {signals}");
    }
}

/// `search_codebase`'s input shape is unmeasured, so every string in it
/// that looks like a path — an absolute or a `~` path — goes through
/// the same `..` and secret checks; an ordinary query stays allowed.
#[test]
fn a_search_path_is_checked_too() {
    let dir = tmp("search");
    for query in [
        "/home/alice/.ssh/id_rsa",
        "~/.aws/credentials",
        "../../etc/passwd",
    ] {
        let payload = call("search_codebase", serde_json::json!({"query": query}));
        let (cancel, _, signals) = run(&dir, &payload);
        assert!(cancel, "{query} was allowed: {signals}");
    }
    for query in ["auth token handling", "fn main", "secret_path"] {
        let payload = call("search_codebase", serde_json::json!({"query": query}));
        let (cancel, _, signals) = run(&dir, &payload);
        assert!(!cancel, "{query} was cancelled: {signals}");
    }
}
