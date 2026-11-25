//! Git command-line global-flag parsing and built-in subcommand list.

/// Parsed git invocation before subcommand execution.
#[derive(Debug, Clone)]
pub(crate) struct GitParsed {
    pub c_dirs: Vec<String>,
    pub git_dir_flags: Vec<String>,
    pub work_tree_flags: Vec<String>,
    pub c_configs: Vec<(String, String)>,
    pub config_envs: Vec<String>,
    pub exec_path_override: bool,
    pub subcmd: Option<String>,
    pub sub_args: Vec<String>,
}

const VALUE_GLOBAL_FLAGS: &[&str] = &[
    "-C",
    "-c",
    "--git-dir",
    "--work-tree",
    "--namespace",
    "--super-prefix",
    "--config-env",
];

/// Parse git global flags up to the first subcommand token.
pub(crate) fn parse_git_invocation(args: &[String]) -> GitParsed {
    let mut parsed = GitParsed {
        c_dirs: Vec::new(),
        git_dir_flags: Vec::new(),
        work_tree_flags: Vec::new(),
        c_configs: Vec::new(),
        config_envs: Vec::new(),
        exec_path_override: false,
        subcmd: None,
        sub_args: Vec::new(),
    };
    let mut i = 0usize;
    while i < args.len() {
        let a = args[i].as_str();
        if a == "--" || a == "--end-of-options" {
            i += 1;
            break;
        }
        if let Some(val) = a.strip_prefix("-C")
            && !val.is_empty()
        {
            parsed.c_dirs.push(val.to_string());
            i += 1;
            continue;
        }
        if let Some(val) = a.strip_prefix("-c")
            && !val.is_empty()
        {
            parsed.c_configs.push(split_kv(val));
            i += 1;
            continue;
        }
        if let Some(val) = a.strip_prefix("--git-dir=") {
            parsed.git_dir_flags.push(val.to_string());
            i += 1;
            continue;
        }
        if let Some(val) = a.strip_prefix("--work-tree=") {
            parsed.work_tree_flags.push(val.to_string());
            i += 1;
            continue;
        }
        if let Some(val) = a.strip_prefix("--config-env=") {
            parsed.config_envs.push(val.to_string());
            i += 1;
            continue;
        }
        if a.starts_with("--exec-path=") {
            parsed.exec_path_override = true;
            i += 1;
            continue;
        }
        if a.starts_with("--namespace=")
            || a.starts_with("--super-prefix=")
            || a.starts_with("--list-cmds=")
        {
            i += 1;
            continue;
        }
        if VALUE_GLOBAL_FLAGS.contains(&a) {
            if let Some(val) = args.get(i + 1) {
                match a {
                    "-C" => parsed.c_dirs.push(val.clone()),
                    "-c" => parsed.c_configs.push(split_kv(val)),
                    "--git-dir" => parsed.git_dir_flags.push(val.clone()),
                    "--work-tree" => parsed.work_tree_flags.push(val.clone()),
                    "--config-env" => parsed.config_envs.push(val.clone()),
                    _ => {}
                }
                i += 2;
                continue;
            }
            i += 1;
            continue;
        }
        if a.starts_with('-') {
            i += 1;
            continue;
        }
        parsed.subcmd = Some(a.to_string());
        parsed.sub_args = args[i + 1..].to_vec();
        return parsed;
    }
    if i < args.len() && parsed.subcmd.is_none() && !args[i].starts_with('-') {
        parsed.subcmd = Some(args[i].clone());
        parsed.sub_args = args[i + 1..].to_vec();
    }
    parsed
}

fn split_kv(raw: &str) -> (String, String) {
    match raw.split_once('=') {
        Some((k, v)) => (k.to_string(), v.to_string()),
        None => (raw.to_string(), String::new()),
    }
}

/// Built-in git commands that git never resolves via `alias.<name>`.
pub(crate) fn is_builtin_git_cmd(sub: &str) -> bool {
    matches!(
        sub,
        "add"
            | "am"
            | "apply"
            | "archive"
            | "bisect"
            | "blame"
            | "branch"
            | "bundle"
            | "cat-file"
            | "check-ignore"
            | "checkout"
            | "cherry-pick"
            | "clean"
            | "clone"
            | "commit"
            | "config"
            | "describe"
            | "diff"
            | "fetch"
            | "for-each-ref"
            | "format-patch"
            | "fsck"
            | "gc"
            | "grep"
            | "help"
            | "init"
            | "log"
            | "ls-files"
            | "ls-remote"
            | "ls-tree"
            | "merge"
            | "merge-base"
            | "mv"
            | "notes"
            | "pull"
            | "push"
            | "range-diff"
            | "rebase"
            | "reflog"
            | "remote"
            | "reset"
            | "restore"
            | "rev-list"
            | "rev-parse"
            | "revert"
            | "rm"
            | "shortlog"
            | "show"
            | "show-ref"
            | "sparse-checkout"
            | "stash"
            | "status"
            | "submodule"
            | "switch"
            | "symbolic-ref"
            | "tag"
            | "update-ref"
            | "version"
            | "whatchanged"
            | "worktree"
    )
}

/// Re-encode global `-C`/`-c`/`--git-dir`/`--work-tree`/`--config-env` flags ahead of an expanded alias.
pub(crate) fn rebuild_global_prefix(parsed: &GitParsed) -> Vec<String> {
    let mut out = Vec::new();
    for d in &parsed.c_dirs {
        out.extend(["-C".into(), d.clone()]);
    }
    for (k, v) in &parsed.c_configs {
        out.push("-c".into());
        if v.is_empty() {
            out.push(k.clone());
        } else {
            out.push(format!("{k}={v}"));
        }
    }
    for gd in &parsed.git_dir_flags {
        out.extend(["--git-dir".into(), gd.clone()]);
    }
    for wt in &parsed.work_tree_flags {
        out.extend(["--work-tree".into(), wt.clone()]);
    }
    for ce in &parsed.config_envs {
        out.extend(["--config-env".into(), ce.clone()]);
    }
    out
}
