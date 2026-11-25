//! Command classification for the outward-action guard (D3):
//! decides whether a shimmed binary call is an outward write and
//! formats the single-line command text (capped at 200 chars).

use std::path::PathBuf;

use crate::guard_git::{GitContext, classify_git, classify_pre_push};

const COMMAND_CAP: usize = 200;

/// 3-way guard classification for outward commands (push-perms spec §2.1).
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum GuardVerdict {
    Allow,
    Card { command: String, rule: String },
    Refuse { command: String, rule: String },
}

/// Full guard classification match including normalized `argv` and `cwd`.
#[derive(Debug, Clone)]
pub(crate) struct GuardMatch {
    pub verdict: GuardVerdict,
    pub argv: Vec<String>,
    pub cwd: PathBuf,
}

/// Classify `(name, args)` using `PushPolicy`, `cwd`, and `env`.
#[cfg(test)]
pub(crate) fn classify_command(
    name: &str,
    args: &[String],
    policy: &backlot_model::event::PushPolicy,
    cwd: &std::path::Path,
    env: &crate::cli::Env,
) -> GuardVerdict {
    let info = crate::guard::load_studio_guard_info(env);
    let ctx = GitContext {
        policy,
        cwd,
        role_dir: info.role_dir.as_deref(),
        work_area: info.work_area.as_deref(),
        repo_root: info.repo_root.as_deref(),
        members: &info.members,
        env,
    };
    classify_with_context(name, args, "", &ctx).verdict
}

/// Classify `(name, args, stdin)` with full studio/role context.
pub(crate) fn classify_with_context(
    name: &str,
    args: &[String],
    stdin: &str,
    ctx: &GitContext<'_>,
) -> GuardMatch {
    if name == "git" {
        let (verdict, argv, eff_cwd) = classify_git(args, ctx);
        return GuardMatch {
            verdict,
            argv,
            cwd: eff_cwd,
        };
    }
    if name == "git-push" {
        let mut argv = Vec::with_capacity(args.len() + 2);
        argv.push("git".into());
        argv.push("push".into());
        argv.extend(args.iter().cloned());
        return GuardMatch {
            verdict: classify_pre_push(args, stdin, ctx),
            argv,
            cwd: ctx.cwd.to_path_buf(),
        };
    }
    let mut argv = Vec::with_capacity(args.len() + 1);
    argv.push(name.to_string());
    argv.extend(args.iter().cloned());
    let verdict = classify_non_git(name, args);
    GuardMatch {
        verdict,
        argv,
        cwd: ctx.cwd.to_path_buf(),
    }
}

/// Format `raw` on one line capped at [`COMMAND_CAP`] characters.
pub(crate) fn cap_command(raw: &str) -> String {
    let single: String = raw
        .chars()
        .map(|c| {
            if matches!(c, '\r' | '\n' | '\0') {
                ' '
            } else {
                c
            }
        })
        .collect();
    single.trim().chars().take(COMMAND_CAP).collect()
}

fn next_positional<'a>(args: &'a [String], value_flags: &[&str]) -> Option<(usize, &'a str)> {
    let mut i = 0;
    while i < args.len() {
        let a = args[i].as_str();
        if a == "--" {
            return args.get(i + 1).map(|s| (i + 1, s.as_str()));
        }
        if value_flags.contains(&a) {
            i += 2;
            continue;
        }
        if a.starts_with('-') {
            i += 1;
            continue;
        }
        return Some((i, a));
    }
    None
}

fn gh_api_is_write(args: &[String]) -> bool {
    const VALUE_FLAGS: &[&str] = &[
        "-H",
        "--header",
        "-q",
        "--jq",
        "-t",
        "--template",
        "--cache",
        "-p",
        "--preview",
        "--hostname",
        "--input",
    ];
    let mut i = 0;
    while i < args.len() {
        let a = args[i].as_str();
        if a == "--" {
            break;
        }
        if a == "-X" || a == "--method" {
            match args.get(i + 1).map(String::as_str) {
                Some(m) if m.eq_ignore_ascii_case("GET") => {
                    i += 2;
                    continue;
                }
                _ => return true,
            }
        }
        if let Some(m) = a.strip_prefix("--method=") {
            if !m.eq_ignore_ascii_case("GET") {
                return true;
            }
            i += 1;
            continue;
        }
        if let Some(m) = a.strip_prefix("-X")
            && !m.is_empty()
        {
            if !m.eq_ignore_ascii_case("GET") {
                return true;
            }
            i += 1;
            continue;
        }
        if a == "--field"
            || a == "--raw-field"
            || a.starts_with("--field=")
            || a.starts_with("--raw-field=")
            || a.starts_with("-f")
            || a.starts_with("-F")
        {
            return true;
        }
        if VALUE_FLAGS.contains(&a) {
            i += 2;
            continue;
        }
        i += 1;
    }
    false
}

fn classify_gh(args: &[String]) -> Option<(&'static str, bool)> {
    const ROOT_VALUE_FLAGS: &[&str] = &["-R", "--repo", "--hostname"];
    let (_, sub) = next_positional(args, ROOT_VALUE_FLAGS)?;
    let idx = args.iter().position(|a| a == sub)?;
    match sub {
        "pr" => {
            let rest = &args[idx + 1..];
            match next_positional(rest, ROOT_VALUE_FLAGS).map(|(_, a)| a) {
                Some("create") => Some(("gh pr create", true)),
                Some("view" | "list" | "status" | "diff" | "checks") => None,
                None if rest.iter().any(|a| a == "--help" || a == "-h") => None,
                _ => Some(("gh write", false)),
            }
        }
        "release" | "repo" | "issue" | "gist" | "secret" => Some(("gh write", false)),
        "workflow" => next_positional(&args[idx + 1..], ROOT_VALUE_FLAGS)
            .is_some_and(|(_, action)| action == "run")
            .then_some(("gh write", false)),
        "api" => gh_api_is_write(&args[idx + 1..]).then_some(("gh write", false)),
        _ => None,
    }
}

fn classify_non_git(name: &str, args: &[String]) -> GuardVerdict {
    let hit = match name {
        "gh" => classify_gh(args),
        "hc" => is_hc_refused(args).then_some(("hc control", false)),
        "socat" | "nc" | "ncat" => (names_server_socket(args) || talks_to_a_unix_socket(args))
            .then_some(("server socket", false)),
        "systemd-run" => Some(("detaching from the role's tree", false)),
        "tmux" => is_tmux_detached(args).then_some(("detaching from the role's tree", false)),
        "backlot" => is_backlot_server(args).then_some(("server control", false)),
        "scp" => Some(("remote transport", false)),
        "rsync" => is_rsync_refused(args).then_some(("remote transport", false)),
        "ssh" => is_ssh_refused(args).then_some(("remote transport", false)),
        "npm" => is_npm_refused(args).then_some(("publish", false)),
        "cargo" => is_cargo_refused(args).then_some(("publish", false)),
        _ => None,
    };
    let Some((rule, is_card)) = hit else {
        return GuardVerdict::Allow;
    };
    let joined = std::iter::once(name)
        .chain(args.iter().map(String::as_str))
        .collect::<Vec<_>>()
        .join(" ");
    let command = cap_command(&joined);
    if is_card {
        GuardVerdict::Card {
            command,
            rule: rule.into(),
        }
    } else {
        GuardVerdict::Refuse {
            command,
            rule: rule.into(),
        }
    }
}

/// Legacy refusal check for `refused_command` callers: returns `Some(cmd)` on
/// both `Refuse` and `Card` for non-git commands.
#[cfg(test)]
pub(crate) fn refused_command(name: &str, args: &[String]) -> Option<String> {
    if name == "git-push" {
        let raw = match args.first().map(|s| s.trim()).filter(|s| !s.is_empty()) {
            Some(remote) => format!("git push {remote}"),
            None => "git push".into(),
        };
        return Some(cap_command(&raw));
    }
    match classify_non_git(name, args) {
        GuardVerdict::Allow => None,
        GuardVerdict::Card { command, .. } | GuardVerdict::Refuse { command, .. } => Some(command),
    }
}

/// Inspect a shell command line (such as `agy`'s `run_command` `CommandLine`)
/// and return the formatted refused command if any segment invokes one.
#[cfg(test)]
pub(crate) fn refused_command_line(line: &str) -> Option<String> {
    crate::guard_shell::refused_command_line(line)
}

fn is_hc_refused(args: &[String]) -> bool {
    const VALUE_FLAGS: &[&str] = &["--session", "-s", "--socket", "--on"];
    let Some((idx, sub)) = next_positional(args, VALUE_FLAGS) else {
        return false;
    };
    match sub {
        "update" | "machine" | "run" | "exec" | "send" | "keys" | "connect" => true,
        "server" => next_positional(&args[idx + 1..], VALUE_FLAGS)
            .is_some_and(|(_, action)| action == "stop"),
        _ => false,
    }
}

/// Words naming the person's queue or the core socket directory: a role
/// that writes them is reaching for the server's own documents.
const SERVER_TOKENS: &[&str] = &[
    "plugin.backlot.",
    "doc.set",
    "herdr-core",
    "HC_SOCKET",
    "HC_TERMINAL_SOCKET",
];

/// The first word naming a server token, if any.
pub(crate) fn server_token_in(words: &[String]) -> Option<&str> {
    words
        .iter()
        .map(String::as_str)
        .find(|w| SERVER_TOKENS.iter().any(|t| w.contains(t)))
}

fn names_server_socket(args: &[String]) -> bool {
    server_token_in(args).is_some()
}

/// `nc`/`ncat` `-U` and socat's `UNIX-*` addresses reach a local socket
/// whatever its path: the core's own socket is one command away.
fn talks_to_a_unix_socket(args: &[String]) -> bool {
    args.iter().any(|a| {
        a == "--unixsock"
            || (a.starts_with('-') && !a.starts_with("--") && a[1..].contains('U'))
            || a == "UNIX"
            || a.starts_with("UNIX-")
    })
}

/// `tmux new -d` / `tmux new-session -d` detach from the role's tree.
fn is_tmux_detached(args: &[String]) -> bool {
    const VALUE_FLAGS: &[&str] = &["-L", "-S", "-f", "-c", "-T"];
    let Some((idx, sub)) = next_positional(args, VALUE_FLAGS) else {
        return false;
    };
    if !matches!(sub, "new" | "new-session") {
        return false;
    }
    args[idx + 1..].iter().any(|a| {
        a == "--detached" || (a.starts_with('-') && !a.starts_with("--") && a[1..].contains('d'))
    })
}

/// `backlot server run|start|stop|adopt|service` is the person's.
fn is_backlot_server(args: &[String]) -> bool {
    const VALUE_FLAGS: &[&str] = &[
        "--session",
        "-s",
        "--socket",
        "--studio",
        "--as",
        "--core",
        "--remote",
    ];
    let Some((idx, sub)) = next_positional(args, VALUE_FLAGS) else {
        return false;
    };
    if sub != "server" {
        return false;
    }
    next_positional(&args[idx + 1..], VALUE_FLAGS)
        .is_some_and(|(_, action)| matches!(action, "run" | "start" | "stop" | "adopt" | "service"))
}

fn has_host_colon(arg: &str) -> bool {
    arg.find(':')
        .is_some_and(|colon| colon > 0 && !arg[..colon].contains('/'))
}

fn is_rsync_refused(args: &[String]) -> bool {
    const VALUE_FLAGS: &[&str] = &["-e", "--rsh"];
    let mut after_dd = false;
    let mut i = 0;
    while i < args.len() {
        let a = args[i].as_str();
        if !after_dd && a == "--" {
            after_dd = true;
            i += 1;
            continue;
        }
        if !after_dd && VALUE_FLAGS.contains(&a) {
            i += 2;
            continue;
        }
        if !after_dd && a.starts_with('-') {
            i += 1;
            continue;
        }
        if has_host_colon(a) {
            return true;
        }
        i += 1;
    }
    false
}

fn is_ssh_refused(args: &[String]) -> bool {
    // OpenSSH options whose single-letter flag consumes an argument.
    const SSH_VAL_OPTS: &str = "BbcDEeFIiJlmOopQRSWw";
    let mut after_dd = false;
    let mut positionals = 0usize;
    let mut i = 0;
    while i < args.len() {
        let a = args[i].as_str();
        if !after_dd && a == "--" {
            after_dd = true;
            i += 1;
            continue;
        }
        if !after_dd && a.starts_with('-') && a.len() > 1 {
            let mut consumes_next = false;
            let mut chars = a[1..].chars();
            while let Some(c) = chars.next() {
                if SSH_VAL_OPTS.contains(c) {
                    if chars.as_str().is_empty() {
                        consumes_next = true;
                    }
                    break;
                }
            }
            i += if consumes_next { 2 } else { 1 };
            continue;
        }
        positionals += 1;
        if positionals >= 2 {
            return true;
        }
        i += 1;
    }
    false
}

fn is_npm_refused(args: &[String]) -> bool {
    const VALUE_FLAGS: &[&str] = &[
        "-C",
        "--prefix",
        "--userconfig",
        "--cache",
        "--registry",
        "--access",
        "--workspace",
        "-w",
        "--tag",
        "--otp",
    ];
    next_positional(args, VALUE_FLAGS).is_some_and(|(_, sub)| sub == "publish")
}

fn is_cargo_refused(args: &[String]) -> bool {
    const VALUE_FLAGS: &[&str] = &[
        "-C",
        "--manifest-path",
        "--target-dir",
        "--color",
        "--config",
        "-Z",
        "--explain",
        "--jobs",
        "-j",
        "--target",
    ];
    let mut i = 0;
    while i < args.len() {
        let a = args[i].as_str();
        if a == "--" {
            return false;
        }
        if a.starts_with('+') {
            i += 1;
            continue;
        }
        if VALUE_FLAGS.contains(&a) {
            i += 2;
            continue;
        }
        if a.starts_with('-') {
            i += 1;
            continue;
        }
        return a == "publish";
    }
    false
}
