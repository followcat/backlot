//! Fail-closed `git push` option and positional parser.

/// Parsed `git push` options and positionals under the fail-closed option allowlist.
#[derive(Debug, Clone, Default)]
pub(crate) struct ParsedPushArgs {
    pub repo_flag: Option<String>,
    pub tags_flag: bool,
    pub follow_tags_flag: bool,
    pub all_flag: bool,
    pub branches_flag: bool,
    pub explicit_recurse_no_or_check: bool,
    pub positional: Vec<String>,
}

/// Fail-closed `git push` option and positional parser.
///
/// Only exact allowlisted long options (and explicit known-refused spellings) and
/// short letters in `{u, q, v, n, 4, 6, o}` are accepted; any abbreviated or
/// unrecognized option returns `Err` so the caller refuses it.
pub(crate) fn parse_push_args(args: &[String]) -> Result<ParsedPushArgs, String> {
    let mut out = ParsedPushArgs::default();
    let mut after_dd = false;
    let mut i = 0usize;
    while i < args.len() {
        let a = args[i].as_str();
        if !after_dd && a == "--" {
            after_dd = true;
            i += 1;
            continue;
        }
        if !after_dd && a.starts_with("--") {
            match a {
                "--no-verify" => return Err("hook bypass (--no-verify)".into()),
                "--exec" => return Err("git push override (--exec)".into()),
                s if s.starts_with("--exec=") => return Err("git push override (--exec)".into()),
                "--receive-pack" => return Err("git push override (--receive-pack)".into()),
                s if s.starts_with("--receive-pack=") => {
                    return Err("git push override (--receive-pack)".into());
                }
                "--upload-pack" => return Err("git push override (--upload-pack)".into()),
                s if s.starts_with("--upload-pack=") => {
                    return Err("git push override (--upload-pack)".into());
                }
                "--force" | "--force-with-lease" | "--force-if-includes" => {
                    return Err("force push".into());
                }
                s if s.starts_with("--force-with-lease=") => return Err("force push".into()),
                "--mirror" => return Err("mirror push".into()),
                "--delete" => return Err("remote delete".into()),
                "--prune" => return Err("remote prune".into()),
                "--recurse-submodules" => {
                    return Err("submodule push (--recurse-submodules)".into());
                }
                s if s.starts_with("--recurse-submodules=") => {
                    let mode = &s["--recurse-submodules=".len()..];
                    if matches!(mode, "no" | "check") {
                        out.explicit_recurse_no_or_check = true;
                        i += 1;
                        continue;
                    }
                    return Err("submodule push (--recurse-submodules)".into());
                }
                "--no-recurse-submodules" => {
                    out.explicit_recurse_no_or_check = true;
                    i += 1;
                    continue;
                }
                "--signed" => return Err("signed push (--signed)".into()),
                s if s.starts_with("--signed=") => {
                    let mode = &s["--signed=".len()..];
                    if matches!(mode, "no" | "false") {
                        i += 1;
                        continue;
                    }
                    return Err("signed push (--signed)".into());
                }
                "--no-signed" => {
                    i += 1;
                    continue;
                }
                "--tags" => out.tags_flag = true,
                "--no-tags" => out.tags_flag = false,
                "--follow-tags" => out.follow_tags_flag = true,
                "--no-follow-tags" => out.follow_tags_flag = false,
                "--all" => out.all_flag = true,
                "--no-all" => out.all_flag = false,
                "--branches" => out.branches_flag = true,
                "--no-branches" => out.branches_flag = false,
                "--repo" => {
                    let Some(val) = args.get(i + 1) else {
                        return Err("missing value for git push option (--repo)".into());
                    };
                    out.repo_flag = Some(val.clone());
                    i += 2;
                    continue;
                }
                s if s.starts_with("--repo=") => {
                    out.repo_flag = Some(s["--repo=".len()..].to_string());
                }
                "--no-repo" => out.repo_flag = None,
                "--push-option" => {
                    if args.get(i + 1).is_none() {
                        return Err("missing value for git push option (--push-option)".into());
                    }
                    i += 2;
                    continue;
                }
                s if s.starts_with("--push-option=") => {}
                "--no-push-option" | "--set-upstream" | "--no-set-upstream" | "--quiet"
                | "--no-quiet" | "--verbose" | "--no-verbose" | "--progress" | "--no-progress"
                | "--dry-run" | "--no-dry-run" | "--porcelain" | "--no-porcelain" | "--atomic"
                | "--no-atomic" | "--ipv4" | "--ipv6" | "--thin" | "--no-thin" | "--verify" => {}
                _ => return Err(format!("unknown git push option ({a})")),
            }
            i += 1;
            continue;
        }
        if !after_dd && a.starts_with('-') {
            if a == "-" {
                return Err("unknown git push option (-)".into());
            }
            let mut extra = 0usize;
            for (idx, ch) in a[1..].char_indices() {
                match ch {
                    'u' | 'q' | 'v' | 'n' | '4' | '6' => {}
                    'o' => {
                        if idx + ch.len_utf8() == a[1..].len() {
                            if args.get(i + 1).is_none() {
                                return Err("missing value for git push option (-o)".into());
                            }
                            extra = 1;
                        }
                        break;
                    }
                    'f' => return Err("force push".into()),
                    'd' => return Err("remote delete".into()),
                    _ => return Err(format!("unknown git push option (-{ch})")),
                }
            }
            i += 1 + extra;
            continue;
        }
        out.positional.push(a.to_string());
        i += 1;
    }
    Ok(out)
}
