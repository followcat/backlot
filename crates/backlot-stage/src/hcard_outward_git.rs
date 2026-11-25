//! Git and GitHub PR metadata inspection and staleness checks for
//! `CardAction::Outward` (split out of `hcard_outward.rs` for file size).

use std::path::Path;

use backlot_model::event::PushPolicy;
use backlot_model::state::StudioConfig;

use crate::cli::Env;
use crate::guard_git::GitContext;
use crate::guard_git_env::run_real_git;
use crate::guard_git_parse::parse_git_invocation;
use crate::guard_git_probe::{
    current_branch, git_config_get, is_local_tag, remote_default_branch, resolve_default_remote,
    resolve_remote_push_urls, url_matches_policy,
};
use crate::hcard_outward_env::stage_probe_env;

pub(crate) fn is_gh_pr_create(argv: &[String]) -> bool {
    argv.first()
        .and_then(|s| Path::new(s).file_name()?.to_str())
        == Some("gh")
        && argv.get(1).map(String::as_str) == Some("pr")
        && argv.get(2).map(String::as_str) == Some("create")
}

pub(crate) fn verify_local_git_clean(cwd: &Path, env: &Env) -> Result<(), String> {
    for key in ["core.sshCommand", "core.hooksPath", "core.fsmonitor"] {
        if let Some((0, val, _)) =
            run_real_git(&["config", "--local", "--get", key], cwd, env, None)
            && !val.trim().is_empty()
        {
            return Err(format!("local git config tampered ({key})"));
        }
    }
    if let Some((0, val, _)) = run_real_git(
        &[
            "config",
            "--local",
            "--get-regexp",
            r"^remote\..*\.receivepack$",
        ],
        cwd,
        env,
        None,
    ) && !val.trim().is_empty()
    {
        return Err("local git config tampered (remote.*.receivepack)".into());
    }
    Ok(())
}

fn flag_opt(args: &[String], short: &str, long: &str) -> Option<String> {
    let eq = format!("{long}=");
    let mut i = 0;
    while i < args.len() {
        let a = args[i].as_str();
        if a == short || a == long {
            return args.get(i + 1).cloned();
        }
        if let Some(v) = a.strip_prefix(&eq) {
            return Some(v.to_string());
        }
        i += 1;
    }
    None
}

pub(crate) fn inspect_gh_pr(
    argv: &[String],
    cwd: &Path,
    ctx: &GitContext<'_>,
    cfg: &StudioConfig,
    head_oid: &str,
) -> Result<(String, String, String, Vec<String>, u32), String> {
    let args = argv.get(3..).unwrap_or(&[]);
    let explicit_head = flag_opt(args, "-H", "--head")
        .map(|h| h.rsplit_once(':').map(|(_, b)| b.to_string()).unwrap_or(h));
    let head_branch = explicit_head
        .or_else(|| current_branch(cwd, ctx))
        .ok_or("cannot determine head branch for gh pr create")?;
    let remote_name = resolve_default_remote(cwd, Some(&head_branch), ctx);
    let urls = resolve_remote_push_urls(&remote_name, cwd, ctx)
        .filter(|u| !u.is_empty())
        .ok_or_else(|| {
            format!(
                "head branch '{head_branch}' is not pushed to recorded remote '{remote_name}' — push it first"
            )
        })?;
    let matched_url = urls
        .iter()
        .find(|u| url_matches_policy(u, cwd, ctx))
        .cloned()
        .ok_or_else(|| format!("remote '{remote_name}' URL is not recorded in studio policy"))?;
    verify_branch_on_remote(
        &cfg.push,
        cwd,
        ctx.env,
        &remote_name,
        &matched_url,
        &head_branch,
        head_oid,
    )?;

    let base_branch = flag_opt(args, "-B", "--base")
        .or_else(|| remote_default_branch(&remote_name, cwd, ctx))
        .unwrap_or_else(|| "main".into());
    let range = rev_range(cwd, ctx.env, &remote_name, &base_branch, head_oid);
    let (commits, commits_total) = collect_commits(cwd, ctx.env, &range);
    Ok((
        matched_url,
        format!("refs/heads/{head_branch}"),
        head_oid.into(),
        commits,
        commits_total,
    ))
}

pub(crate) fn verify_branch_on_remote(
    policy: &PushPolicy,
    cwd: &Path,
    clean: &Env,
    remote_name: &str,
    remote_url: &str,
    head_branch: &str,
    head_oid: &str,
) -> Result<(), String> {
    let msg = || {
        format!(
            "head branch '{head_branch}' is not pushed to recorded remote '{remote_name}' — push it first"
        )
    };
    if !policy.has_recorded_url(remote_url, |_| true)
        && !url_recorded_via_git(policy, cwd, clean, remote_url)
    {
        return Err(msg());
    }
    let ref_full = format!("refs/heads/{head_branch}");
    if let Some((0, out, _)) = run_real_git(
        &["ls-remote", "--heads", remote_url, &ref_full],
        cwd,
        clean,
        None,
    ) {
        let oid = out
            .lines()
            .next()
            .and_then(|l| l.split_whitespace().next())
            .unwrap_or("");
        return if oid == head_oid { Ok(()) } else { Err(msg()) };
    }
    let tracking = format!("refs/remotes/{remote_name}/{head_branch}");
    let Some((0, tr_out, _)) =
        run_real_git(&["rev-parse", "--verify", &tracking], cwd, clean, None)
    else {
        return Err(msg());
    };
    if tr_out.trim() != head_oid {
        return Err(msg());
    }
    Ok(())
}

fn url_recorded_via_git(policy: &PushPolicy, cwd: &Path, clean: &Env, url: &str) -> bool {
    let ctx = GitContext {
        policy,
        cwd,
        role_dir: None,
        work_area: None,
        repo_root: None,
        members: &[],
        env: clean,
    };
    url_matches_policy(url, cwd, &ctx)
}

pub(crate) fn inspect_git_push(
    args: &[String],
    cwd: &Path,
    ctx: &GitContext<'_>,
    head_oid: &str,
) -> Result<(String, String, String, Vec<String>, u32), String> {
    let parsed = parse_git_invocation(args);
    let mut repo_flag = None;
    let mut pos = Vec::new();
    let mut i = 0;
    let mut after_dd = false;
    while i < parsed.sub_args.len() {
        let a = parsed.sub_args[i].as_str();
        if !after_dd && a == "--" {
            after_dd = true;
        } else if !after_dd && a == "--repo" {
            repo_flag = parsed.sub_args.get(i + 1).cloned();
            i += 2;
            continue;
        } else if !after_dd && let Some(r) = a.strip_prefix("--repo=") {
            repo_flag = Some(r.to_string());
        } else if !after_dd && matches!(a, "-o" | "--push-option") {
            i += 2;
            continue;
        } else if after_dd || !a.starts_with('-') {
            pos.push(a.to_string());
        }
        i += 1;
    }
    let cur = current_branch(cwd, ctx);
    let remote = pos
        .first()
        .cloned()
        .or(repo_flag)
        .unwrap_or_else(|| resolve_default_remote(cwd, cur.as_deref(), ctx));
    let remote_url = resolve_remote_push_urls(&remote, cwd, ctx)
        .and_then(|u| u.into_iter().next())
        .unwrap_or_else(|| remote.clone());
    let first_spec = pos.get(1).map(String::as_str);
    let (src_spec, dst_ref) = match first_spec {
        Some(rs) if rs.contains(':') => {
            let (s, d) = rs.split_once(':').unwrap();
            let src = if s.is_empty() { "HEAD" } else { s };
            let dst = if d.starts_with("refs/") {
                d.to_string()
            } else if is_local_tag(src, cwd, ctx) {
                format!("refs/tags/{d}")
            } else {
                format!("refs/heads/{d}")
            };
            (src.to_string(), dst)
        }
        Some("HEAD") | None => {
            let b = cur.clone().unwrap_or_else(|| "HEAD".into());
            let dst = git_config_get(&format!("branch.{b}.merge"), cwd, ctx)
                .unwrap_or_else(|| format!("refs/heads/{b}"));
            ("HEAD".into(), dst)
        }
        Some(s) if is_local_tag(s, cwd, ctx) => (s.into(), format!("refs/tags/{s}")),
        Some(s) => {
            let dst = if s.starts_with("refs/") {
                s.into()
            } else {
                format!("refs/heads/{s}")
            };
            (s.into(), dst)
        }
    };
    let src_oid = run_real_git(&["rev-parse", &src_spec], cwd, ctx.env, None)
        .and_then(|(c, out, _)| (c == 0).then(|| out.trim().to_string()))
        .unwrap_or_else(|| head_oid.into());
    let mut remote_oid = String::new();
    if let Some(branch) = dst_ref.strip_prefix("refs/heads/") {
        let tr = format!("refs/remotes/{remote}/{branch}");
        if let Some((0, out, _)) = run_real_git(&["rev-parse", "--verify", &tr], cwd, ctx.env, None)
        {
            remote_oid = out.trim().to_string();
        }
    }
    let range = if !remote_oid.is_empty() {
        format!("{remote_oid}..{src_oid}")
    } else if let Some(b) = remote_default_branch(&remote, cwd, ctx) {
        rev_range(cwd, ctx.env, &remote, &b, &src_oid)
    } else {
        src_oid.clone()
    };
    let (commits, commits_total) = collect_commits(cwd, ctx.env, &range);
    Ok((remote_url, dst_ref, remote_oid, commits, commits_total))
}

fn rev_range(cwd: &Path, clean: &Env, remote: &str, base: &str, head: &str) -> String {
    let remote_base = format!("refs/remotes/{remote}/{base}");
    if run_real_git(&["rev-parse", "--verify", &remote_base], cwd, clean, None)
        .is_some_and(|(c, _, _)| c == 0)
    {
        return format!("{remote_base}..{head}");
    }
    if run_real_git(&["rev-parse", "--verify", base], cwd, clean, None)
        .is_some_and(|(c, _, _)| c == 0)
    {
        return format!("{base}..{head}");
    }
    head.into()
}

fn collect_commits(cwd: &Path, clean: &Env, range: &str) -> (Vec<String>, u32) {
    let total = run_real_git(&["rev-list", "--count", range], cwd, clean, None)
        .and_then(|(c, out, _)| (c == 0).then(|| out.trim().parse::<u32>().ok()).flatten())
        .unwrap_or(0);
    let lines = run_real_git(&["log", "--oneline", "-n", "20", range], cwd, clean, None)
        .and_then(|(c, out, _)| {
            (c == 0).then(|| {
                out.lines()
                    .map(str::trim)
                    .filter(|l| !l.is_empty())
                    .map(String::from)
                    .collect()
            })
        })
        .unwrap_or_default();
    let _ = stage_probe_env;
    (lines, total)
}
