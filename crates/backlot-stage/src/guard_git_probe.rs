//! Git repository queries for push classification and `pre-push` hook
//! verification (split out of `guard_git_push.rs` for file size).

use std::path::Path;

use crate::guard_git::{GitContext, common_dir_of};
use crate::guard_git_env::{normalize_resolve, run_real_git};

pub(crate) fn url_matches_policy(url: &str, eff_cwd: &Path, ctx: &GitContext<'_>) -> bool {
    let cur_common = common_dir_of(eff_cwd, ctx.env, ctx.role_dir);
    ctx.policy.has_recorded_url(url, |rec_repo| {
        if let Some(ref cur) = cur_common
            && let Some(rec_cmn) = common_dir_of(Path::new(rec_repo), ctx.env, ctx.role_dir)
        {
            return *cur == rec_cmn;
        }
        normalize_resolve(eff_cwd, Path::new(rec_repo)) == normalize_resolve(eff_cwd, eff_cwd)
    })
}

pub(crate) fn current_branch(eff_cwd: &Path, ctx: &GitContext<'_>) -> Option<String> {
    let (0, out, _) = run_real_git(
        &["symbolic-ref", "--quiet", "--short", "HEAD"],
        eff_cwd,
        ctx.env,
        ctx.role_dir,
    )?
    else {
        return None;
    };
    let s = out.trim();
    (!s.is_empty()).then(|| s.to_string())
}

pub(crate) fn git_config_get(key: &str, eff_cwd: &Path, ctx: &GitContext<'_>) -> Option<String> {
    let (0, out, _) = run_real_git(&["config", "--get", key], eff_cwd, ctx.env, ctx.role_dir)?
    else {
        return None;
    };
    let s = out.trim();
    (!s.is_empty()).then(|| s.to_string())
}

pub(crate) fn configured_remote_push(
    remote: &str,
    eff_cwd: &Path,
    ctx: &GitContext<'_>,
) -> Option<Vec<String>> {
    let (0, out, _) = run_real_git(
        &["config", "--get-all", &format!("remote.{remote}.push")],
        eff_cwd,
        ctx.env,
        ctx.role_dir,
    )?
    else {
        return None;
    };
    let specs: Vec<String> = out
        .lines()
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(String::from)
        .collect();
    (!specs.is_empty()).then_some(specs)
}

pub(crate) fn resolve_default_remote(
    eff_cwd: &Path,
    cur: Option<&str>,
    ctx: &GitContext<'_>,
) -> String {
    if let Some(b) = cur
        && let Some(r) = git_config_get(&format!("branch.{b}.pushRemote"), eff_cwd, ctx)
    {
        return r;
    }
    if let Some(r) = git_config_get("remote.pushDefault", eff_cwd, ctx) {
        return r;
    }
    if let Some(b) = cur
        && let Some(r) = git_config_get(&format!("branch.{b}.remote"), eff_cwd, ctx)
    {
        return r;
    }
    "origin".into()
}

pub(crate) fn resolve_remote_push_urls(
    remote: &str,
    eff_cwd: &Path,
    ctx: &GitContext<'_>,
) -> Option<Vec<String>> {
    let (0, out, _) = run_real_git(
        &["remote", "get-url", "--all", "--push", remote],
        eff_cwd,
        ctx.env,
        ctx.role_dir,
    )?
    else {
        return None;
    };
    Some(
        out.lines()
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(String::from)
            .collect(),
    )
}

pub(crate) fn remote_default_branch(
    remote: &str,
    eff_cwd: &Path,
    ctx: &GitContext<'_>,
) -> Option<String> {
    let ref_name = format!("refs/remotes/{remote}/HEAD");
    let (0, out, _) = run_real_git(
        &["symbolic-ref", "--quiet", &ref_name],
        eff_cwd,
        ctx.env,
        ctx.role_dir,
    )?
    else {
        return None;
    };
    let prefix = format!("refs/remotes/{remote}/");
    out.trim().strip_prefix(&prefix).map(String::from)
}

pub(crate) fn is_local_tag(name: &str, eff_cwd: &Path, ctx: &GitContext<'_>) -> bool {
    if name.starts_with("refs/tags/") {
        return true;
    }
    run_real_git(
        &[
            "show-ref",
            "--verify",
            "--quiet",
            &format!("refs/tags/{name}"),
        ],
        eff_cwd,
        ctx.env,
        ctx.role_dir,
    )
    .is_some_and(|(code, _, _)| code == 0)
}
