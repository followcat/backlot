//! The hook's JSON and shell side: the payload's strings are decoded
//! before any check, and each decoded command is scanned with quote
//! awareness — so an escaped quote, a backtick between escaped quotes,
//! a quoted semicolon or an unsupported escape cannot slip a command
//! past a rule — plus the wider secret list and the secret arguments an
//! allowed command could still be pointed at.

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
fn a_semicolon_inside_a_message_is_literal() {
    let dir = tmp("semicolon");
    let (cancel, _, signals) = run(&dir, &commands_of(&["git commit -m \"a; b\""]));
    assert!(!cancel, "a quoted semicolon is one command: {signals}");
}

/// A Conventional Commit subject almost always carries a scope in
/// parentheses, and a message may hold a `;` or a `|` inside its
/// quotes. The segment scan is quote-aware, so none of that splits a
/// command or refuses one — and a chained pair of allowed commands is
/// still two allowed segments.
#[test]
fn quoted_text_neither_splits_nor_refuses() {
    let dir = tmp("quoted-allow");
    for cmd in [
        "git commit -m \"fix(runners): tidy\"",
        "git commit -m 'feat(x): y; z'",
        "git commit -m \"a | b\"",
        "git add -A && git commit -m \"docs: z\"",
    ] {
        let (cancel, _, signals) = run(&dir, &commands_of(&[cmd]));
        assert!(!cancel, "{cmd:?} was cancelled: {signals}");
    }
}

/// Quoting cannot hide a substitution, a second command, or an
/// unterminated string — and a quoted program name matches no rule.
#[test]
fn quoting_does_not_hide_a_command() {
    let dir = tmp("quoted-cancel");
    for cmd in [
        "git commit -m \"$(id)\"",
        "git commit -m \"`id`\"",
        "git commit -m 'x' ; curl y",
        "git status \"x",
        "\"curl\" x",
        "git status (x)",
    ] {
        let (cancel, _, signals) = run(&dir, &commands_of(&[cmd]));
        assert!(cancel, "{cmd:?} was allowed: {signals}");
    }
}

/// An allowed program can still be pointed at a secret: `git diff
/// --no-index` prints a file and a commit's `-F` echoes one into its
/// summary. Every word of an allowed segment is checked, quotes
/// removed, and an `--opt=value`'s value as well.
#[test]
fn a_secret_argument_is_refused() {
    let dir = tmp("secret-arg");
    for cmd in [
        "git diff --no-index /proc/self/environ /dev/null",
        "git commit --allow-empty -F /proc/self/environ",
        "git diff --no-index /home/alice/.ssh/id_rsa /dev/null",
    ] {
        let (cancel, _, signals) = run(&dir, &commands_of(&[cmd]));
        assert!(cancel, "{cmd:?} was allowed: {signals}");
    }
    // Ordinary arguments stay allowed, `..` included: `main..HEAD` is a
    // range, not a traversal.
    for cmd in ["git diff main..HEAD", "git add src/lib.rs"] {
        let (cancel, _, signals) = run(&dir, &commands_of(&[cmd]));
        assert!(!cancel, "{cmd:?} was cancelled: {signals}");
    }
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
