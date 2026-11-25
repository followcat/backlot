//! Push classification, URL/refspec resolution, and `pre-push` hook
//! verification (split out of `guard_git.rs` for file size).

use std::path::Path;

use crate::guard_git::{GitContext, with_alias};
use crate::guard_git_env::{run_real_git, verify_hook_integrity};
use crate::guard_git_parse::GitParsed;
use crate::guard_git_probe::{
    configured_remote_push, current_branch, git_config_get, is_local_tag, remote_default_branch,
    resolve_default_remote, resolve_remote_push_urls, url_matches_policy,
};
use crate::guard_rules::{GuardVerdict, cap_command};

/// Classify a resolved `git push` invocation against `PushPolicy` and recorded remotes.
pub(crate) fn classify_push(
    parsed: &GitParsed,
    eff_cwd: &Path,
    ctx: &GitContext<'_>,
    raw_cmd: &str,
    via_alias: Option<&str>,
) -> GuardVerdict {
    let refuse = |rule: String| GuardVerdict::Refuse {
        command: raw_cmd.to_string(),
        rule: with_alias(rule, via_alias),
    };
    let card = |rule: String| GuardVerdict::Card {
        command: raw_cmd.to_string(),
        rule: with_alias(rule, via_alias),
    };

    if let Err(rule) = verify_hook_integrity(eff_cwd, ctx.env, ctx.role_dir) {
        return refuse(rule);
    }
    if !parsed.c_configs.is_empty() {
        return refuse("git config override on push (-c)".into());
    }
    if !parsed.config_envs.is_empty() {
        return refuse("git config override on push (--config-env)".into());
    }
    if parsed.exec_path_override {
        return refuse("git exec-path override on push".into());
    }

    let mut repo_flag: Option<String> = None;
    let mut tags_flag = false;
    let mut all_flag = false;
    let mut pos: Vec<String> = Vec::new();
    let mut after_dd = false;
    let mut i = 0usize;
    while i < parsed.sub_args.len() {
        let a = parsed.sub_args[i].as_str();
        if !after_dd && a == "--" {
            after_dd = true;
            i += 1;
            continue;
        }
        if !after_dd && a.starts_with('-') {
            if a == "--no-verify" {
                return refuse("hook bypass (--no-verify)".into());
            }
            if a == "--exec" || a.starts_with("--exec=") {
                return refuse("git push override (--exec)".into());
            }
            if a == "--receive-pack" || a.starts_with("--receive-pack=") {
                return refuse("git push override (--receive-pack)".into());
            }
            if a == "--upload-pack" || a.starts_with("--upload-pack=") {
                return refuse("git push override (--upload-pack)".into());
            }
            if a == "--force"
                || a == "--force-with-lease"
                || a.starts_with("--force-with-lease=")
                || a == "--force-if-includes"
            {
                return refuse("force push".into());
            }
            if a == "--mirror" {
                return refuse("mirror push".into());
            }
            if a == "--delete" {
                return refuse("remote delete".into());
            }
            if a == "--prune" {
                return refuse("remote prune".into());
            }
            if a == "--tags" || a == "--follow-tags" {
                tags_flag = true;
                i += 1;
                continue;
            }
            if a == "--all" || a == "--branches" {
                all_flag = true;
                i += 1;
                continue;
            }
            if a == "--repo" {
                repo_flag = parsed.sub_args.get(i + 1).cloned();
                i += 2;
                continue;
            }
            if let Some(r) = a.strip_prefix("--repo=") {
                repo_flag = Some(r.to_string());
                i += 1;
                continue;
            }
            if a == "-o" || a == "--push-option" {
                i += 2;
                continue;
            }
            if !a.starts_with("--") && !a.starts_with("-o") {
                for ch in a[1..].chars() {
                    if ch == 'f' {
                        return refuse("force push".into());
                    }
                    if ch == 'd' {
                        return refuse("remote delete".into());
                    }
                }
            }
            i += 1;
            continue;
        }
        pos.push(a.to_string());
        i += 1;
    }

    let refspecs: &[String] = if pos.len() > 1 { &pos[1..] } else { &[] };
    for rs in refspecs {
        if rs.starts_with('+') {
            return refuse("force push".into());
        }
        if rs.starts_with(':') {
            return refuse("remote delete".into());
        }
    }

    let cur_branch = current_branch(eff_cwd, ctx);
    let remote = pos
        .first()
        .cloned()
        .or(repo_flag)
        .unwrap_or_else(|| resolve_default_remote(eff_cwd, cur_branch.as_deref(), ctx));

    let Some(urls) = resolve_remote_push_urls(&remote, eff_cwd, ctx) else {
        return card(format!("unrecorded remote URL '{remote}'"));
    };
    if urls.is_empty() {
        return card(format!("unrecorded remote URL '{remote}'"));
    }
    for url in &urls {
        if !url_matches_policy(url, eff_cwd, ctx) {
            return card(format!("unrecorded remote URL '{url}'"));
        }
    }

    if tags_flag && ctx.policy.protect_tags {
        return card("tag push".into());
    }
    if all_flag {
        return card("pushes all local branches".into());
    }

    let specs_owned: Vec<String>;
    let effective_specs: &[String] = if !refspecs.is_empty() {
        refspecs
    } else if let Some(configured) = configured_remote_push(&remote, eff_cwd, ctx) {
        specs_owned = configured;
        for rs in &specs_owned {
            if rs.starts_with('+') {
                return refuse("force push".into());
            }
            if rs.starts_with(':') {
                return refuse("remote delete".into());
            }
        }
        &specs_owned
    } else {
        if git_config_get("push.default", eff_cwd, ctx).as_deref() == Some("matching") {
            return card("push.default=matching".into());
        }
        let Some(ref cur) = cur_branch else {
            return card("detached HEAD push".into());
        };
        let dst = git_config_get(&format!("branch.{cur}.merge"), eff_cwd, ctx)
            .unwrap_or_else(|| format!("refs/heads/{cur}"));
        return check_target_ref(&dst, false, &remote, eff_cwd, ctx, &card);
    };

    for rs in effective_specs {
        let v = classify_refspec(rs, &cur_branch, &remote, eff_cwd, ctx, &card);
        if !matches!(v, GuardVerdict::Allow) {
            return v;
        }
    }
    GuardVerdict::Allow
}

fn classify_refspec(
    rs: &str,
    cur_branch: &Option<String>,
    remote: &str,
    eff_cwd: &Path,
    ctx: &GitContext<'_>,
    card: &dyn Fn(String) -> GuardVerdict,
) -> GuardVerdict {
    if let Some((src, dst)) = rs.split_once(':') {
        if dst.is_empty() {
            return card(format!("ambiguous ref '{rs}'"));
        }
        if dst.starts_with("refs/tags/") {
            return if ctx.policy.protect_tags {
                card("tag push".into())
            } else {
                GuardVerdict::Allow
            };
        }
        if let Some(branch) = dst.strip_prefix("refs/heads/") {
            return check_branch_protection(branch, remote, eff_cwd, ctx, card);
        }
        if dst.starts_with("refs/") {
            return card(format!("non-branch ref '{dst}'"));
        }
        if is_local_tag(src, eff_cwd, ctx) {
            return if ctx.policy.protect_tags {
                card("tag push".into())
            } else {
                GuardVerdict::Allow
            };
        }
        return check_branch_protection(dst, remote, eff_cwd, ctx, card);
    }
    if rs == "HEAD" {
        let Some(cur) = cur_branch else {
            return card("detached HEAD push".into());
        };
        let dst = if git_config_get("push.default", eff_cwd, ctx).as_deref() == Some("upstream") {
            git_config_get(&format!("branch.{cur}.merge"), eff_cwd, ctx)
                .unwrap_or_else(|| format!("refs/heads/{cur}"))
        } else {
            format!("refs/heads/{cur}")
        };
        return check_target_ref(&dst, false, remote, eff_cwd, ctx, card);
    }
    if is_local_tag(rs, eff_cwd, ctx) {
        return if ctx.policy.protect_tags {
            card("tag push".into())
        } else {
            GuardVerdict::Allow
        };
    }
    if rs.starts_with("refs/") && !rs.starts_with("refs/heads/") {
        return card(format!("non-branch ref '{rs}'"));
    }
    let branch = rs.strip_prefix("refs/heads/").unwrap_or(rs);
    check_branch_protection(branch, remote, eff_cwd, ctx, card)
}

fn check_target_ref(
    dst: &str,
    is_tag: bool,
    remote: &str,
    eff_cwd: &Path,
    ctx: &GitContext<'_>,
    card: &dyn Fn(String) -> GuardVerdict,
) -> GuardVerdict {
    if is_tag || dst.starts_with("refs/tags/") {
        return if ctx.policy.protect_tags {
            card("tag push".into())
        } else {
            GuardVerdict::Allow
        };
    }
    if dst.starts_with("refs/") && !dst.starts_with("refs/heads/") {
        return card(format!("non-branch ref '{dst}'"));
    }
    let branch = dst.strip_prefix("refs/heads/").unwrap_or(dst);
    check_branch_protection(branch, remote, eff_cwd, ctx, card)
}

fn check_branch_protection(
    branch: &str,
    remote: &str,
    eff_cwd: &Path,
    ctx: &GitContext<'_>,
    card: &dyn Fn(String) -> GuardVerdict,
) -> GuardVerdict {
    if let Some(pat) = ctx.policy.matched_protected_branch(branch) {
        return card(format!("protected branch '{pat}'"));
    }
    if ctx.policy.protect_default_branch
        && remote_default_branch(remote, eff_cwd, ctx).as_deref() == Some(branch)
    {
        return card(format!("remote default branch '{branch}'"));
    }
    GuardVerdict::Allow
}

/// Classify a `pre-push` hook invocation (`backlot guard git-push -- <remote> <url>`).
pub(crate) fn classify_pre_push(
    args: &[String],
    stdin: &str,
    ctx: &GitContext<'_>,
) -> GuardVerdict {
    let remote_name = args.first().map(|s| s.trim()).unwrap_or("");
    let remote_url = args
        .get(1)
        .map(|s| s.trim())
        .filter(|s| !s.is_empty())
        .unwrap_or(remote_name);
    let raw_cmd = if remote_name.is_empty() {
        "git push".to_string()
    } else {
        cap_command(&format!("git push {remote_name}"))
    };
    let refuse = |rule: String| GuardVerdict::Refuse {
        command: raw_cmd.clone(),
        rule,
    };
    let card = |rule: String| GuardVerdict::Card {
        command: raw_cmd.clone(),
        rule,
    };

    if remote_url.is_empty() || !url_matches_policy(remote_url, ctx.cwd, ctx) {
        return card(format!("unrecorded remote URL '{remote_url}'"));
    }

    let zero_oid = "0".repeat(40);
    let mut first_card: Option<GuardVerdict> = None;

    for line in stdin.lines().map(str::trim).filter(|l| !l.is_empty()) {
        let parts: Vec<&str> = line.split_whitespace().collect();
        if parts.len() < 4 {
            continue;
        }
        let (_local_ref, local_oid, remote_ref, remote_oid) =
            (parts[0], parts[1], parts[2], parts[3]);
        if local_oid == zero_oid {
            return refuse("remote delete".into());
        }
        if remote_oid != zero_oid {
            let ok = run_real_git(
                &["merge-base", "--is-ancestor", remote_oid, local_oid],
                ctx.cwd,
                ctx.env,
                ctx.role_dir,
            )
            .is_some_and(|(code, _, _)| code == 0);
            if !ok {
                return refuse("non-fast-forward push".into());
            }
        }
        let ref_verdict = check_target_ref(
            remote_ref,
            remote_ref.starts_with("refs/tags/"),
            remote_name,
            ctx.cwd,
            ctx,
            &card,
        );
        if !matches!(ref_verdict, GuardVerdict::Allow) && first_card.is_none() {
            first_card = Some(ref_verdict);
        }
    }

    first_card.unwrap_or(GuardVerdict::Allow)
}
