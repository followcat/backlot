//! Read-only probe exemptions and noisy-probe suppression helpers for the
//! outward-action guard (`guard.rs`, `guard_git.rs`).

use std::fs::{self, OpenOptions};
use std::os::unix::io::AsRawFd;
use std::path::{Path, PathBuf};

use crate::cli::Env;
use crate::design_doc::atomic_write;
use crate::guard::{SHIM_MARKER, studio_role_from_env};
use crate::guard_git::GitContext;
use crate::guard_git_env::check_git_config_env;
use crate::guard_git_parse::{GitParsed, parse_git_invocation};

const SUPPRESSED_FILE: &str = "suppressed-probes";
const SUPPRESSED_LOCK: &str = "suppressed-probes.lock";

fn has_git_env_path(env: &Env) -> bool {
    ["GIT_DIR", "GIT_WORK_TREE", "GIT_COMMON_DIR"]
        .iter()
        .any(|k| env.get(k).is_some_and(|s| !s.trim().is_empty()))
}

fn has_external_git_config_file(env: &Env) -> bool {
    ["GIT_CONFIG", "GIT_CONFIG_GLOBAL", "GIT_CONFIG_SYSTEM"]
        .iter()
        .any(|k| {
            env.get(k)
                .map(str::trim)
                .is_some_and(|v| !v.is_empty() && v != "/dev/null")
        })
}

fn has_no_git_overrides(parsed: &GitParsed, env: &Env) -> bool {
    parsed.c_dirs.is_empty()
        && parsed.git_dir_flags.is_empty()
        && parsed.work_tree_flags.is_empty()
        && parsed.c_configs.is_empty()
        && parsed.config_envs.is_empty()
        && !parsed.exec_path_override
        && parsed.unknown_global_opt.is_none()
        && !has_git_env_path(env)
}

/// Non-repository git metadata queries (`--version`, `-v`, `--help`, `-h`,
/// `version`, `help`, path help flags) that neither read nor modify a repo.
pub(crate) fn is_benign_git_meta(args: &[String], parsed: &GitParsed, env: &Env) -> bool {
    if !has_no_git_overrides(parsed, env) {
        return false;
    }
    if matches!(parsed.subcmd.as_deref(), Some("version" | "help")) {
        return true;
    }
    if parsed.subcmd.is_some() || args.is_empty() {
        return false;
    }
    let mut saw_meta = false;
    for a in args {
        if a == "--" {
            return false;
        }
        if matches!(
            a.as_str(),
            "-v" | "--version" | "-h" | "--help" | "--html-path" | "--man-path" | "--info-path"
        ) {
            saw_meta = true;
        } else {
            return false;
        }
    }
    saw_meta
}

/// Layout-only `git rev-parse` probes (`--show-toplevel`, `--git-dir`,
/// `--is-inside-work-tree`, ...) without `-C`, `--git-dir`, `--work-tree`,
/// or `GIT_DIR`/`GIT_WORK_TREE` escapes.
pub(crate) fn is_benign_rev_parse_probe(parsed: &GitParsed, ctx: &GitContext<'_>) -> bool {
    if parsed.subcmd.as_deref() != Some("rev-parse")
        || !has_no_git_overrides(parsed, ctx.env)
        || has_external_git_config_file(ctx.env)
        || check_git_config_env(ctx.env, ctx.cwd, ctx.role_dir, &[]).is_err()
        || parsed.sub_args.is_empty()
    {
        return false;
    }
    parsed.sub_args.iter().all(|a| {
        matches!(
            a.as_str(),
            "--show-toplevel"
                | "--git-dir"
                | "--git-common-dir"
                | "--absolute-git-dir"
                | "--is-inside-work-tree"
                | "--is-inside-git-dir"
                | "--is-bare-repository"
                | "--is-shallow-repository"
                | "--show-prefix"
                | "--show-cdup"
                | "--show-superproject-working-tree"
                | "--shared-index-path"
                | "--local-env-vars"
                | "-q"
                | "--quiet"
        ) || a.starts_with("--path-format=")
    })
}

/// Pure local read-only git subcommands allowed across a studio's own worktrees
/// and bound repository directories.
pub(crate) fn is_read_only_git_cmd(parsed: &GitParsed) -> bool {
    if parsed
        .sub_args
        .iter()
        .any(|a| a == "--output" || a.starts_with("--output="))
    {
        return false;
    }
    match parsed.subcmd.as_deref() {
        Some(
            "status" | "log" | "show" | "diff" | "diff-index" | "diff-files" | "diff-tree"
            | "rev-parse" | "rev-list" | "ls-files" | "ls-tree" | "for-each-ref" | "show-ref"
            | "merge-base" | "cat-file" | "describe" | "shortlog" | "blame" | "grep"
            | "check-ignore" | "count-objects" | "name-rev" | "cherry" | "range-diff" | "version"
            | "help",
        ) => true,
        Some("branch") => {
            let mutates = parsed.sub_args.iter().any(|a| {
                matches!(
                    a.as_str(),
                    "-d" | "-D"
                        | "-m"
                        | "-M"
                        | "-c"
                        | "-C"
                        | "-f"
                        | "--delete"
                        | "--move"
                        | "--copy"
                        | "--force"
                        | "--set-upstream-to"
                        | "--unset-upstream"
                        | "--edit-description"
                        | "--create-reflog"
                        | "-t"
                        | "--track"
                ) || a.starts_with("--set-upstream-to=")
            });
            let reads = parsed.sub_args.is_empty()
                || parsed.sub_args.iter().any(|a| {
                    matches!(
                        a.as_str(),
                        "--list"
                            | "--show-current"
                            | "-l"
                            | "-a"
                            | "--all"
                            | "-r"
                            | "--remotes"
                            | "-v"
                            | "-vv"
                            | "--verbose"
                            | "--contains"
                            | "--no-contains"
                            | "--merged"
                            | "--no-merged"
                    )
                });
            !mutates && reads
        }
        Some("remote") => {
            let mut pos = parsed
                .sub_args
                .iter()
                .filter(|a| !a.starts_with('-'))
                .map(String::as_str);
            matches!(pos.next(), None | Some("get-url"))
        }
        Some("config") => {
            let is_read = parsed.sub_args.iter().any(|a| {
                matches!(
                    a.as_str(),
                    "--get" | "--get-all" | "--get-regexp" | "--list" | "-l"
                )
            });
            let is_write = parsed.sub_args.iter().any(|a| {
                matches!(
                    a.as_str(),
                    "--add"
                        | "--unset"
                        | "--unset-all"
                        | "--replace-all"
                        | "--edit"
                        | "-e"
                        | "--rename-section"
                        | "--remove-section"
                )
            });
            is_read && !is_write
        }
        _ => false,
    }
}

/// Resolve the studio directory from env or a role's `<studio>/roles/<role>` path.
pub(crate) fn studio_dir_from(role_dir: Option<&Path>, env: &Env) -> Option<PathBuf> {
    if let Some((sdir, _)) = studio_role_from_env(env) {
        return Some(sdir);
    }
    let rd = role_dir?;
    let roles_dir = rd.parent()?;
    (roles_dir.file_name().and_then(|n| n.to_str()) == Some("roles"))
        .then(|| roles_dir.parent().map(Path::to_path_buf))
        .flatten()
}

fn is_benign_config_override_rule(rule: &str) -> bool {
    let key_raw = rule
        .strip_prefix("git config override in GIT_CONFIG_PARAMETERS (")
        .or_else(|| rule.strip_prefix("git config override in GIT_CONFIG_* ("))
        .and_then(|rest| rest.strip_suffix(')'));
    let Some(key) = key_raw else {
        return false;
    };
    let lower = key.trim().to_ascii_lowercase();
    matches!(
        lower.as_str(),
        "core.fsmonitor" | "core.hookspath" | "submodule.recurse"
    ) || lower.starts_with("alias.")
}

/// Whether a still-refused command is a harmless read-only git probe whose
/// `role.refused` and `outward action` events should be suppressed.
pub(crate) fn is_suppressed_probe_refusal(env: &Env, argv: &[String], rule: Option<&str>) -> bool {
    if argv.first().map(String::as_str) != Some("git") {
        return false;
    }
    let Some(rule) = rule else {
        return false;
    };
    if rule.contains("via alias") {
        return false;
    }
    let parsed = parse_git_invocation(&argv[1..]);
    if parsed.subcmd.as_deref() == Some("push")
        || !parsed
            .subcmd
            .as_deref()
            .is_none_or(|_| is_read_only_git_cmd(&parsed))
    {
        return false;
    }
    if !parsed.c_dirs.is_empty()
        || !parsed.git_dir_flags.is_empty()
        || !parsed.work_tree_flags.is_empty()
        || parsed.exec_path_override
        || has_git_env_path(env)
        || has_external_git_config_file(env)
    {
        return false;
    }
    rule == "git hooksPath tampered"
        || rule == "git path outside work area"
        || rule.starts_with("unknown git global option (")
        || is_benign_config_override_rule(rule)
}

/// Atomically increment the studio's suppressed probe refusal counter.
pub(crate) fn record_suppressed_probe(sdir: &Path) -> Result<u64, String> {
    let _ = fs::create_dir_all(sdir);
    let lock_f = OpenOptions::new()
        .create(true)
        .truncate(false)
        .read(true)
        .write(true)
        .open(sdir.join(SUPPRESSED_LOCK))
        .map_err(|e| format!("suppressed-probes lock: {e}"))?;
    #[cfg(unix)]
    {
        let rc = unsafe { libc::flock(lock_f.as_raw_fd(), libc::LOCK_EX) };
        if rc != 0 {
            return Err(format!("suppressed-probes flock failed ({rc})"));
        }
    }
    let next = read_suppressed_count(sdir).saturating_add(1);
    atomic_write(&sdir.join(SUPPRESSED_FILE), format!("{next}\n").as_bytes())
        .map_err(|e| format!("suppressed-probes write: {e}"))?;
    #[cfg(unix)]
    unsafe {
        libc::flock(lock_f.as_raw_fd(), libc::LOCK_UN);
    }
    drop(lock_f);
    let _ = SHIM_MARKER;
    Ok(next)
}

/// Read the studio's current suppressed probe refusal total (`0` if absent).
pub(crate) fn read_suppressed_count(sdir: &Path) -> u64 {
    fs::read_to_string(sdir.join(SUPPRESSED_FILE))
        .ok()
        .and_then(|s| s.trim().parse::<u64>().ok())
        .unwrap_or(0)
}

pub(crate) fn add_studio_read_roots(
    ctx: &GitContext<'_>,
    work_roots: &mut Vec<PathBuf>,
    git_roots: &mut Vec<PathBuf>,
) {
    use crate::guard_git_env::normalize_resolve;
    if let Some(sdir) = studio_dir_from(ctx.role_dir, ctx.env) {
        let sw = normalize_resolve(ctx.cwd, &sdir.join("work"));
        work_roots.push(sw.clone());
        git_roots.push(sw);
    }
    if let Some(rr) = ctx.repo_root {
        let nrr = normalize_resolve(ctx.cwd, rr);
        work_roots.push(nrr.clone());
        git_roots.push(nrr);
    }
    for m in ctx.members {
        let nm = normalize_resolve(ctx.cwd, Path::new(m));
        work_roots.push(nm.clone());
        git_roots.push(nm);
    }
}
