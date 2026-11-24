//! Command classification for the outward-action guard (D3):
//! decides whether a shimmed binary call is an outward write and
//! formats the single-line command text (capped at 200 chars).

const COMMAND_CAP: usize = 200;

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

fn is_gh_refused(args: &[String]) -> bool {
    const ROOT_VALUE_FLAGS: &[&str] = &["-R", "--repo", "--hostname"];
    let Some((idx, sub)) = next_positional(args, ROOT_VALUE_FLAGS) else {
        return false;
    };
    match sub {
        "pr" | "release" | "repo" | "issue" | "gist" | "secret" => true,
        "workflow" => next_positional(&args[idx + 1..], ROOT_VALUE_FLAGS)
            .is_some_and(|(_, action)| action == "run"),
        "api" => gh_api_is_write(&args[idx + 1..]),
        _ => false,
    }
}

fn is_hc_refused(args: &[String]) -> bool {
    const VALUE_FLAGS: &[&str] = &["--session", "-s", "--socket"];
    let Some((idx, sub)) = next_positional(args, VALUE_FLAGS) else {
        return false;
    };
    match sub {
        "update" | "machine" => true,
        "server" => next_positional(&args[idx + 1..], VALUE_FLAGS)
            .is_some_and(|(_, action)| action == "stop"),
        _ => false,
    }
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

/// If `(name, args)` is an outward command this guard refuses, return
/// its formatted command text (capped at 200 chars); otherwise `None`.
pub(crate) fn refused_command(name: &str, args: &[String]) -> Option<String> {
    if name == "git-push" {
        let raw = match args.first().map(|s| s.trim()).filter(|s| !s.is_empty()) {
            Some(remote) => format!("git push {remote}"),
            None => "git push".into(),
        };
        return Some(cap_command(&raw));
    }
    let refused = match name {
        "gh" => is_gh_refused(args),
        "hc" => is_hc_refused(args),
        "scp" => true,
        "rsync" => is_rsync_refused(args),
        "ssh" => is_ssh_refused(args),
        "npm" => is_npm_refused(args),
        "cargo" => is_cargo_refused(args),
        _ => false,
    };
    if !refused {
        return None;
    }
    let joined = std::iter::once(name)
        .chain(args.iter().map(String::as_str))
        .collect::<Vec<_>>()
        .join(" ");
    Some(cap_command(&joined))
}
