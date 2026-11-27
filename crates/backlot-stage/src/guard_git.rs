//! Git-aware outward guard classification, alias expansion, and
//! remote probing (push-perms spec §2.1, Holes 1 & 2).

use std::path::{Path, PathBuf};

use backlot_model::event::{PushPolicy, RecordedRemote};

use crate::cli::Env;
use crate::guard_git_env::{
    check_git_config_env, effective_cwd, normalize_resolve, path_inside, run_real_git,
    split_plain_alias,
};
use crate::guard_git_parse::{
    GitParsed, is_builtin_git_cmd, parse_git_invocation, rebuild_global_prefix,
};
use crate::guard_rules::{GuardVerdict, cap_command};

pub(crate) use crate::guard_git_push::classify_pre_push;

/// Context for classifying git and outward commands against the active studio's
/// `PushPolicy` and role work area.
#[derive(Debug, Clone)]
pub(crate) struct GitContext<'a> {
    pub policy: &'a PushPolicy,
    pub cwd: &'a Path,
    pub role_dir: Option<&'a Path>,
    pub work_area: Option<&'a Path>,
    pub repo_root: Option<&'a Path>,
    pub members: &'a [String],
    pub env: &'a Env,
}

/// Probe remote push URLs for the studio's bound repository (or workspace members).
pub(crate) fn probe_remotes(repo: &str, members: &[String], no_git: bool) -> Vec<RecordedRemote> {
    if no_git {
        return Vec::new();
    }
    if members.is_empty() {
        return probe_repo_remotes(Path::new(repo), None);
    }
    let mut out = Vec::new();
    for m in members {
        out.extend(probe_repo_remotes(Path::new(m), Some(m.clone())));
    }
    out
}

fn probe_repo_remotes(repo_dir: &Path, recorded_repo: Option<String>) -> Vec<RecordedRemote> {
    if !repo_dir.is_dir() || !crate::paths::is_git(repo_dir) {
        return Vec::new();
    }
    let env = Env::default();
    let Some((0, names, _)) = run_real_git(&["remote"], repo_dir, &env, None) else {
        return Vec::new();
    };
    let mut out = Vec::new();
    for name in names.lines().map(str::trim).filter(|s| !s.is_empty()) {
        if let Some((0, urls, _)) = run_real_git(
            &["remote", "get-url", "--all", "--push", name],
            repo_dir,
            &env,
            None,
        ) {
            for url in urls.lines().map(str::trim).filter(|s| !s.is_empty()) {
                out.push(RecordedRemote {
                    repo: recorded_repo.clone(),
                    name: name.to_string(),
                    url: url.to_string(),
                });
            }
        }
    }
    out
}

fn allowed_roots(
    ctx: &GitContext<'_>,
    eff_cwd: &Path,
    read_only: bool,
) -> (Vec<PathBuf>, Vec<PathBuf>, bool) {
    let mut work_roots = Vec::new();
    let mut git_roots = Vec::new();
    let enforce_work = match ctx.work_area {
        Some(wa) if wa.exists() || ctx.repo_root.is_some_and(|r| r.exists()) => {
            let nwa = normalize_resolve(ctx.cwd, wa);
            work_roots.push(nwa.clone());
            git_roots.push(nwa.clone());
            if let Some(cmn) = common_dir_of(wa, ctx.env, ctx.role_dir) {
                git_roots.push(cmn);
            }
            if let Some(rr) = ctx.repo_root {
                let nrr = normalize_resolve(ctx.cwd, rr);
                git_roots.push(nrr.join(".git"));
            }
            for m in ctx.members {
                let nm = normalize_resolve(ctx.cwd, Path::new(m));
                git_roots.push(nm.join(".git"));
                if let Some(cmn) = common_dir_of(Path::new(m), ctx.env, ctx.role_dir) {
                    git_roots.push(cmn);
                }
            }
            if read_only {
                crate::guard_probes::add_studio_read_roots(ctx, &mut work_roots, &mut git_roots);
            }
            true
        }
        _ if ctx.role_dir.is_some() => {
            let ncwd = normalize_resolve(ctx.cwd, ctx.cwd);
            work_roots.push(ncwd.clone());
            git_roots.push(ncwd);
            if let Some(cmn) = common_dir_of(eff_cwd, ctx.env, ctx.role_dir) {
                git_roots.push(cmn);
            }
            if read_only {
                crate::guard_probes::add_studio_read_roots(ctx, &mut work_roots, &mut git_roots);
            }
            false
        }
        _ => false,
    };
    (work_roots, git_roots, enforce_work)
}

pub(crate) fn common_dir_of(dir: &Path, env: &Env, role_dir: Option<&Path>) -> Option<PathBuf> {
    if !dir.is_dir() {
        return None;
    }
    let mut clean_env = Env {
        vars: env.vars.clone(),
    };
    clean_env.vars.remove("GIT_DIR");
    clean_env.vars.remove("GIT_WORK_TREE");
    clean_env.vars.remove("GIT_COMMON_DIR");
    let (0, out, _) = run_real_git(
        &["rev-parse", "--path-format=absolute", "--git-common-dir"],
        dir,
        &clean_env,
        role_dir,
    )?
    else {
        return None;
    };
    let p = Path::new(out.trim());
    if p.as_os_str().is_empty() {
        None
    } else {
        Some(normalize_resolve(dir, p))
    }
}

fn check_containment(
    ctx: &GitContext<'_>,
    parsed: &GitParsed,
    eff_cwd: &Path,
) -> Result<(), String> {
    let read_only = crate::guard_probes::is_read_only_git_cmd(parsed);
    let (work_roots, git_roots, enforce_cwd) = allowed_roots(ctx, eff_cwd, read_only);
    check_git_config_env(ctx.env, eff_cwd, ctx.role_dir, &work_roots)?;
    if work_roots.is_empty() {
        return Ok(());
    }
    let neff = normalize_resolve(ctx.cwd, eff_cwd);
    if (enforce_cwd || !parsed.c_dirs.is_empty())
        && !path_inside(&neff, &work_roots)
        && !path_inside(&neff, &git_roots)
    {
        return Err("git path outside work area".into());
    }
    for wt in parsed
        .work_tree_flags
        .iter()
        .map(String::as_str)
        .chain(ctx.env.get("GIT_WORK_TREE").filter(|s| !s.is_empty()))
    {
        let np = normalize_resolve(eff_cwd, Path::new(wt));
        if !path_inside(&np, &work_roots) {
            return Err("git path outside work area".into());
        }
    }
    for gd in parsed
        .git_dir_flags
        .iter()
        .map(String::as_str)
        .chain(ctx.env.get("GIT_DIR").filter(|s| !s.is_empty()))
        .chain(ctx.env.get("GIT_COMMON_DIR").filter(|s| !s.is_empty()))
    {
        let np = normalize_resolve(eff_cwd, Path::new(gd));
        if !path_inside(&np, &git_roots) {
            return Err("git path outside work area".into());
        }
    }
    Ok(())
}

/// Fast path check for non-push `git` invocations: no ledger I/O when `true`.
pub(crate) fn is_fast_path(args: &[String], ctx: &GitContext<'_>) -> bool {
    let parsed = parse_git_invocation(args);
    if crate::guard_probes::is_benign_git_meta(args, &parsed, ctx.env)
        || crate::guard_probes::is_benign_rev_parse_probe(&parsed, ctx)
    {
        return true;
    }
    if parsed.exec_path_override
        || parsed.unknown_global_opt.is_some()
        || parsed
            .c_configs
            .iter()
            .any(|(k, _)| k.eq_ignore_ascii_case("core.hooksPath"))
    {
        return false;
    }
    let eff = effective_cwd(ctx.cwd, &parsed.c_dirs);
    if check_containment(ctx, &parsed, &eff).is_err() {
        return false;
    }
    match parsed.subcmd.as_deref() {
        None => true,
        Some("push") => false,
        Some(sub) if is_builtin_git_cmd(sub) => true,
        Some(sub) => lookup_alias(sub, &parsed, &eff, ctx).is_none(),
    }
}

fn lookup_alias(
    sub: &str,
    parsed: &GitParsed,
    eff_cwd: &Path,
    ctx: &GitContext<'_>,
) -> Option<String> {
    let mut cmd_args: Vec<String> = Vec::new();
    for (k, v) in &parsed.c_configs {
        cmd_args.push("-c".into());
        if v.is_empty() {
            cmd_args.push(k.clone());
        } else {
            cmd_args.push(format!("{k}={v}"));
        }
    }
    cmd_args.extend(["config".into(), "--get".into(), format!("alias.{sub}")]);
    let refs: Vec<&str> = cmd_args.iter().map(String::as_str).collect();
    let (code, out, _) = run_real_git(&refs, eff_cwd, ctx.env, ctx.role_dir)?;
    (code == 0).then(|| out.trim_end_matches(['\r', '\n']).to_string())
}

/// Classify a `git <args...>` invocation (PATH shim / `agy` hook).
pub(crate) fn classify_git(
    args: &[String],
    ctx: &GitContext<'_>,
) -> (GuardVerdict, Vec<String>, PathBuf) {
    let raw_cmd = cap_command(&format!("git {}", args.join(" ")));
    let mut cur_args = args.to_vec();
    let mut via_alias: Option<String> = None;
    let mut depth = 0usize;

    loop {
        let parsed = parse_git_invocation(&cur_args);
        if depth == 0
            && (crate::guard_probes::is_benign_git_meta(&cur_args, &parsed, ctx.env)
                || crate::guard_probes::is_benign_rev_parse_probe(&parsed, ctx))
        {
            return (
                GuardVerdict::Allow,
                with_git_prefix(&cur_args),
                ctx.cwd.to_path_buf(),
            );
        }
        let eff = effective_cwd(ctx.cwd, &parsed.c_dirs);
        let pre_err = check_containment(ctx, &parsed, &eff)
            .err()
            .or_else(|| {
                parsed
                    .exec_path_override
                    .then(|| "git exec-path override".into())
            })
            .or_else(|| {
                parsed
                    .unknown_global_opt
                    .as_ref()
                    .map(|opt| format!("unknown git global option ({opt})"))
            })
            .or_else(|| {
                parsed
                    .c_configs
                    .iter()
                    .any(|(k, _)| k.eq_ignore_ascii_case("core.hooksPath"))
                    .then(|| "git hooksPath tampered".into())
            });
        if let Some(rule) = pre_err {
            return (
                GuardVerdict::Refuse {
                    command: raw_cmd,
                    rule: with_alias(rule, via_alias.as_deref()),
                },
                with_git_prefix(&cur_args),
                eff,
            );
        }
        let Some(sub) = parsed.subcmd.clone() else {
            return (GuardVerdict::Allow, with_git_prefix(&cur_args), eff);
        };
        if sub == "push" {
            let verdict = crate::guard_git_push::classify_push(
                &parsed,
                &eff,
                ctx,
                &raw_cmd,
                via_alias.as_deref(),
            );
            return (verdict, with_git_prefix(&cur_args), eff);
        }
        if is_builtin_git_cmd(&sub) {
            return (GuardVerdict::Allow, with_git_prefix(&cur_args), eff);
        }
        let Some(alias_val) = lookup_alias(&sub, &parsed, &eff, ctx) else {
            return (GuardVerdict::Allow, with_git_prefix(&cur_args), eff);
        };
        let alias_name = via_alias.get_or_insert_with(|| sub.clone()).clone();
        let trimmed = alias_val.trim_start();
        if let Some(shell_body) = trimmed.strip_prefix('!') {
            return classify_shell_alias(
                shell_body,
                &parsed.sub_args,
                &eff,
                ctx,
                &raw_cmd,
                &alias_name,
            );
        }
        depth += 1;
        if depth > 4 {
            return (
                GuardVerdict::Refuse {
                    command: raw_cmd,
                    rule: format!("git alias recursion limit via alias '{alias_name}'"),
                },
                with_git_prefix(&cur_args),
                eff,
            );
        }
        let Ok(expanded) = split_plain_alias(trimmed) else {
            return (
                GuardVerdict::Refuse {
                    command: raw_cmd,
                    rule: format!("unparseable git alias via alias '{alias_name}'"),
                },
                with_git_prefix(&cur_args),
                eff,
            );
        };
        let mut rebuilt = rebuild_global_prefix(&parsed);
        rebuilt.extend(expanded);
        rebuilt.extend(parsed.sub_args);
        cur_args = rebuilt;
    }
}

fn classify_shell_alias(
    shell_body: &str,
    sub_args: &[String],
    eff_cwd: &Path,
    ctx: &GitContext<'_>,
    raw_cmd: &str,
    alias_name: &str,
) -> (GuardVerdict, Vec<String>, PathBuf) {
    let mut line = shell_body.to_string();
    if !sub_args.is_empty() && !shell_body.contains('$') {
        let quoted: Vec<String> = sub_args
            .iter()
            .map(|a| format!("'{}'", a.replace('\'', "'\\''")))
            .collect();
        line.push(' ');
        line.push_str(&quoted.join(" "));
    }
    let m = crate::guard_shell::classify_command_line_match(
        &line,
        ctx.policy,
        eff_cwd,
        ctx.env,
        ctx.role_dir,
        ctx.work_area,
        ctx.repo_root,
        ctx.members,
    );
    let verdict = match m.verdict {
        GuardVerdict::Allow => GuardVerdict::Allow,
        GuardVerdict::Card { rule, .. } => GuardVerdict::Card {
            command: raw_cmd.to_string(),
            rule: with_alias(rule, Some(alias_name)),
        },
        GuardVerdict::Refuse { rule, .. } => GuardVerdict::Refuse {
            command: raw_cmd.to_string(),
            rule: with_alias(rule, Some(alias_name)),
        },
    };
    let argv = if m.argv.is_empty() {
        with_git_prefix(sub_args)
    } else {
        m.argv
    };
    (verdict, argv, eff_cwd.to_path_buf())
}

fn with_git_prefix(args: &[String]) -> Vec<String> {
    let mut out = Vec::with_capacity(args.len() + 1);
    out.push("git".into());
    out.extend(args.iter().cloned());
    out
}

pub(crate) fn with_alias(rule: String, alias: Option<&str>) -> String {
    match alias {
        Some(a) if !rule.contains(&format!("alias '{a}'")) => format!("{rule} via alias '{a}'"),
        _ => rule,
    }
}
