//! Shell command-line parsing for hook-based outward-action guards and `!` git aliases.

use std::path::{Path, PathBuf};

#[cfg(test)]
use backlot_model::event::PushPolicy;

#[cfg(test)]
use crate::cli::Env;
use crate::guard_git::{GitContext, classify_git};
use crate::guard_rules::{GuardMatch, GuardVerdict, cap_command, classify_with_context};
use crate::guard_shell_lex::split_shell_segments_checked;

/// Inspect a shell command line and return the formatted command if refused or carded.
#[cfg(test)]
pub(crate) fn refused_command_line(line: &str) -> Option<String> {
    let env = Env::default();
    let policy = PushPolicy::default();
    match classify_command_line(line, &policy, Path::new("."), &env) {
        GuardVerdict::Allow => None,
        GuardVerdict::Card { command, .. } | GuardVerdict::Refuse { command, .. } => Some(command),
    }
}

/// Classify a shell command line under the 3-way guard model.
#[cfg(test)]
pub(crate) fn classify_command_line(
    line: &str,
    policy: &PushPolicy,
    cwd: &Path,
    env: &Env,
) -> GuardVerdict {
    classify_command_line_with_ctx(line, &GitContext::bare(policy, cwd, env)).verdict
}

/// Classify a shell command line with full role/studio context, returning `GuardMatch`.
pub(crate) fn classify_command_line_with_ctx(line: &str, ctx: &GitContext<'_>) -> GuardMatch {
    classify_line_depth(line, 0, ctx.cwd, ctx)
}

fn classify_line_depth(line: &str, depth: usize, cwd: &Path, ctx: &GitContext<'_>) -> GuardMatch {
    let parsed = match split_shell_segments_checked(line) {
        Ok(p) => p,
        Err(reason) => {
            return GuardMatch {
                verdict: GuardVerdict::Refuse {
                    command: cap_command(line),
                    rule: reason.into(),
                },
                argv: Vec::new(),
                cwd: cwd.to_path_buf(),
            };
        }
    };

    let mut eff_cwd = cwd.to_path_buf();
    let mut best_card: Option<GuardMatch> = None;

    let mut consider = |m: GuardMatch| -> Option<GuardMatch> {
        match &m.verdict {
            GuardVerdict::Refuse { .. } => Some(m),
            GuardVerdict::Card { .. } => {
                if best_card.is_none() {
                    best_card = Some(m);
                }
                None
            }
            GuardVerdict::Allow => None,
        }
    };

    for sub in &parsed.nested {
        if depth < 4 {
            let m = classify_line_depth(sub, depth + 1, &eff_cwd, ctx);
            if let Some(refused) = consider(m) {
                return refused;
            }
        }
    }

    let mut heredoc: Option<String> = None;
    for seg in &parsed.segments {
        if seg.iter().any(|t| t.contains("BACKLOT_")) {
            return GuardMatch {
                verdict: GuardVerdict::Refuse {
                    command: cap_command(&seg.join(" ")),
                    rule: "touches BACKLOT_* environment".into(),
                },
                argv: seg.clone(),
                cwd: eff_cwd,
            };
        }
        if let Some(delimiter) = heredoc.as_deref() {
            if seg.len() == 1 && seg[0] == delimiter {
                heredoc = None;
                continue;
            }
            if interpreter_code_names_server(&seg.join(" ")) {
                return GuardMatch {
                    verdict: GuardVerdict::Refuse {
                        command: cap_command(&seg.join(" ")),
                        rule: "server socket or queue write".into(),
                    },
                    argv: seg.clone(),
                    cwd: eff_cwd,
                };
            }
            continue;
        }
        if ctx.role_dir.is_some() || ctx.env.get("BACKLOT_ROLE").is_some_and(|s| !s.is_empty()) {
            let role = ctx.env.get("BACKLOT_ROLE").filter(|s| !s.is_empty());
            if let Some(rule) = crate::proc_guard::check_segment_self_approval(seg, role) {
                return GuardMatch {
                    verdict: GuardVerdict::Refuse {
                        command: cap_command(&seg.join(" ")),
                        rule,
                    },
                    argv: seg.clone(),
                    cwd: eff_cwd,
                };
            }
        }
        if let Some((delimiter, code)) = interpreter_code(seg) {
            if interpreter_code_names_server(&code) {
                return GuardMatch {
                    verdict: GuardVerdict::Refuse {
                        command: cap_command(&seg.join(" ")),
                        rule: "server socket or queue write".into(),
                    },
                    argv: seg.clone(),
                    cwd: eff_cwd,
                };
            }
            heredoc = delimiter;
        }
        let m = eval_segment(seg, depth, &mut eff_cwd, ctx);
        if let Some(refused) = consider(m) {
            return refused;
        }
    }

    best_card.unwrap_or(GuardMatch {
        verdict: GuardVerdict::Allow,
        argv: Vec::new(),
        cwd: eff_cwd,
    })
}

fn eval_segment(
    seg: &[String],
    depth: usize,
    eff_cwd: &mut PathBuf,
    ctx: &GitContext<'_>,
) -> GuardMatch {
    let mut i = 0;
    while i < seg.len() {
        let tok = seg[i].as_str();
        if is_env_assign(tok) {
            i += 1;
            continue;
        }
        let base = tok.rsplit('/').next().unwrap_or(tok);
        if base == "env" {
            i += 1;
            while i < seg.len() {
                let flag = seg[i].as_str();
                if flag == "--" {
                    i += 1;
                    break;
                }
                if matches!(flag, "-u" | "-C" | "-S" | "--unset" | "--chdir") {
                    i += 2;
                } else if flag.starts_with('-') {
                    i += 1;
                } else {
                    break;
                }
            }
            continue;
        }
        if tok == "command" {
            i += 1;
            if seg
                .get(i)
                .is_some_and(|f| f.starts_with("-v") || f.starts_with("-V"))
            {
                return GuardMatch {
                    verdict: GuardVerdict::Allow,
                    argv: Vec::new(),
                    cwd: eff_cwd.clone(),
                };
            }
            continue;
        }
        if matches!(tok, "exec" | "nohup" | "time") {
            i += 1;
            continue;
        }
        break;
    }
    let Some(cmd) = seg.get(i) else {
        return GuardMatch {
            verdict: GuardVerdict::Allow,
            argv: Vec::new(),
            cwd: eff_cwd.clone(),
        };
    };
    let base = cmd.rsplit('/').next().unwrap_or(cmd);
    let rest = &seg[i + 1..];
    if base == "cd" {
        if let Some(dir) = rest.first()
            && !dir.starts_with('-')
        {
            let p = Path::new(dir);
            *eff_cwd = if p.is_absolute() {
                p.to_path_buf()
            } else {
                eff_cwd.join(p)
            };
        }
        return GuardMatch {
            verdict: GuardVerdict::Allow,
            argv: Vec::new(),
            cwd: eff_cwd.clone(),
        };
    }
    match base {
        "sh" | "bash" | "zsh" | "dash" | "ash" if depth < 4 => {
            eval_nested_shell(rest, depth + 1, eff_cwd, ctx)
        }
        "eval" if depth < 4 => classify_line_depth(&rest.join(" "), depth + 1, eff_cwd, ctx),
        "git" => {
            let mut seg_ctx = ctx.clone();
            seg_ctx.cwd = eff_cwd;
            let (verdict, argv, cwd_used) = classify_git(rest, &seg_ctx);
            GuardMatch {
                verdict,
                argv,
                cwd: cwd_used,
            }
        }
        _ => {
            let mut seg_ctx = ctx.clone();
            seg_ctx.cwd = eff_cwd;
            classify_with_context(base, rest, "", &seg_ctx)
        }
    }
}

fn eval_nested_shell(
    rest: &[String],
    next_depth: usize,
    eff_cwd: &Path,
    ctx: &GitContext<'_>,
) -> GuardMatch {
    let mut j = 0;
    while j < rest.len() {
        let arg = rest[j].as_str();
        if arg == "--" || !arg.starts_with('-') {
            break;
        }
        if arg == "-o" || arg == "+o" {
            j += 2;
            continue;
        }
        if !arg.starts_with("--")
            && arg[1..].chars().all(|c| c.is_ascii_alphabetic())
            && arg.contains('c')
        {
            if let Some(script) = rest.get(j + 1) {
                return classify_line_depth(script, next_depth, eff_cwd, ctx);
            }
            break;
        }
        j += 1;
    }
    GuardMatch {
        verdict: GuardVerdict::Allow,
        argv: Vec::new(),
        cwd: eff_cwd.to_path_buf(),
    }
}

fn is_env_assign(tok: &str) -> bool {
    let Some((name, _)) = tok.split_once('=') else {
        return false;
    };
    let mut chars = name.chars();
    let Some(first) = chars.next() else {
        return false;
    };
    (first.is_ascii_alphabetic() || first == '_')
        && chars.all(|c| c.is_ascii_alphanumeric() || c == '_')
}

/// Programs whose inline code or stdin can reach the server's socket.
const INTERPRETERS: &[&str] = &[
    "python", "python3", "node", "perl", "ruby", "deno", "bun", "php", "lua",
];

/// The index of a segment's program, past env assignments and the
/// `env`/`command`/`exec`/`nohup`/`time` wrappers `eval_segment` unwraps.
fn program_index(seg: &[String]) -> Option<usize> {
    let mut i = 0;
    while i < seg.len() {
        let tok = seg[i].as_str();
        if is_env_assign(tok) {
            i += 1;
            continue;
        }
        let base = tok.rsplit('/').next().unwrap_or(tok);
        if base == "env" {
            i += 1;
            while i < seg.len() {
                let flag = seg[i].as_str();
                if flag == "--" {
                    i += 1;
                    break;
                }
                if matches!(flag, "-u" | "-C" | "-S" | "--unset" | "--chdir") {
                    i += 2;
                } else if flag.starts_with('-') {
                    i += 1;
                } else {
                    break;
                }
            }
            continue;
        }
        if matches!(base, "command" | "exec" | "nohup" | "time") {
            i += 1;
            continue;
        }
        return Some(i);
    }
    None
}

/// The code an interpreter segment runs: `Some((heredoc delimiter, code))`
/// for `-c`/`-e`/`--eval` inline code, a here-string, or a heredoc whose
/// body follows in later segments.
fn interpreter_code(seg: &[String]) -> Option<(Option<String>, String)> {
    let idx = program_index(seg)?;
    let base = seg[idx].rsplit('/').next().unwrap_or(&seg[idx]);
    if !INTERPRETERS.contains(&base) {
        return None;
    }
    let args = &seg[idx + 1..];
    for (i, a) in args.iter().enumerate() {
        if a == "--eval"
            || a == "-c"
            || a == "-e"
            || (a.starts_with('-')
                && !a.starts_with("--")
                && (a[1..].contains('c') || a[1..].contains('e')))
        {
            return Some((None, args[i + 1..].join(" ")));
        }
        if let Some(code) = a.strip_prefix("<<<") {
            return Some(if code.is_empty() {
                (None, args[i + 1..].join(" "))
            } else {
                (None, code.to_owned())
            });
        }
        if let Some(rest) = a.strip_prefix("<<") {
            let delimiter = if rest.is_empty() {
                args.get(i + 1).map(String::as_str).unwrap_or("")
            } else {
                rest
            };
            return Some((
                Some(delimiter.trim_start_matches('-').to_owned()),
                String::new(),
            ));
        }
    }
    Some((None, String::new()))
}

/// Interpreter code that writes the person's queue or reaches the core
/// socket. Plain command words (`grep doc.set`, `git commit -m "doc.set: …"`)
/// never reach this check.
fn interpreter_code_names_server(code: &str) -> bool {
    if code.contains("plugin.backlot.req") || code.contains("doc.set") {
        return true;
    }
    let socket_dir = code.contains("herdr-core")
        && (code.contains(".sock")
            || code.contains("/run/user/")
            || code.contains("XDG_RUNTIME_DIR"));
    let socket_var = ["HC_SOCKET", "HC_TERMINAL_SOCKET"].iter().any(|var| {
        code.contains(var)
            && ["os.environ", "process.env", "$HC_", "ENV[", "getenv"]
                .iter()
                .any(|read| code.contains(read))
    });
    socket_dir || socket_var
}
