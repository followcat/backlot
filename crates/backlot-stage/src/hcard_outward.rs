//! Stage-side proposal, `gh pr create` precondition, staleness checking,
//! and preview formatting for `CardAction::Outward` (push-perms spec §2.2).

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use backlot_model::event::{EventKind, OutwardOutcome};
use backlot_model::hollywood::{Card, CardAction, CardBase, HollywoodKind, NewHollywoodEvent};
use backlot_model::state::{OutwardRequestRecord, StudioState};
use backlot_model::{EventBy, NewEvent};

use crate::cli::{Caller, Env, Fail};
use crate::daemon::Stage;
use crate::guard_git::GitContext;
use crate::guard_git_env::{normalize_resolve, path_inside, run_real_git};
use crate::guard_git_probe::resolve_default_remote;
use crate::guard_rules::{GuardVerdict, classify_with_context};
use crate::hcard_outward_git::{
    inspect_gh_pr, inspect_git_push, is_gh_pr_create, verify_branch_on_remote,
    verify_local_git_clean,
};
use crate::store::{self, HPending, Pending};

pub(crate) fn stage_probe_env() -> Env {
    let mut vars = BTreeMap::new();
    for k in [
        "PATH",
        "HOME",
        "USER",
        "LOGNAME",
        "LANG",
        "LC_ALL",
        "TERM",
        "TMPDIR",
        "SSH_AUTH_SOCK",
        "GH_TOKEN",
        "GITHUB_TOKEN",
        "GH_HOST",
    ] {
        if let Ok(v) = std::env::var(k) {
            vars.insert(k.into(), v);
        }
    }
    vars.insert("GIT_CONFIG_COUNT".into(), "0".into());
    vars.insert("GIT_TERMINAL_PROMPT".into(), "0".into());
    vars.insert("GH_PROMPT_DISABLED".into(), "1".into());
    Env { vars }
}

/// Validate `backlot ask --run` at CLI time (`classify == Card`).
pub(crate) fn validate_ask_run(
    sdir: &Path,
    state: &StudioState,
    caller: &Caller,
    env: &Env,
    argv: &[String],
) -> Result<(String, PathBuf), Fail> {
    if argv.is_empty() {
        return Err(Fail::usage("missing command after --run"));
    }
    let cfg = state
        .studio
        .as_ref()
        .ok_or_else(|| Fail::refused("no studio recorded yet; run 'backlot studio new' first"))?;
    let work_area = crate::workareas::role_dir(sdir, &cfg.repo, &caller.role, cfg.no_git);
    let cur = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
    let eff_cwd = if work_area.is_dir()
        && !path_inside(
            &normalize_resolve(&cur, &cur),
            &[normalize_resolve(&cur, &work_area)],
        ) {
        work_area.clone()
    } else {
        cur
    };
    let role_dir = crate::guard::role_dir_from_env(env);
    let ctx = GitContext {
        policy: &cfg.push,
        cwd: &eff_cwd,
        role_dir: role_dir.as_deref(),
        work_area: Some(&work_area),
        repo_root: Some(Path::new(&cfg.repo)),
        members: &cfg.members,
        env,
    };
    let m = classify_with_context(&argv[0], &argv[1..], "", &ctx);
    match m.verdict {
        GuardVerdict::Card { command, .. } => Ok((command, eff_cwd)),
        GuardVerdict::Allow => Err(Fail::refused(format!(
            "'{}' is already allowed without a card",
            argv.join(" ")
        ))),
        GuardVerdict::Refuse { command, rule } => Err(Fail::refused(format!(
            "refused by backlot: {command} cannot run via a card ({rule})"
        ))),
    }
}

impl Stage {
    /// Propose `CardAction::Outward` cards for uncarded `role.refused (Card)`
    /// or `outward.requested` events across all known studios.
    pub(crate) fn propose_outward(&self, hdir: &Path) -> Result<(), Fail> {
        let loaded;
        let studios: &BTreeMap<String, StudioState> = if !self.cached.is_empty() {
            &self.cached
        } else {
            let names = store::studios(&self.base).map_err(|e| Fail::refused(e.to_string()))?;
            let mut map = BTreeMap::new();
            for name in names {
                let sdir = store::studio_dir(&self.base, &name);
                if let Ok(st) = store::load_state(&sdir) {
                    map.insert(name, st);
                }
            }
            loaded = map;
            &loaded
        };
        for (studio_name, state) in studios {
            if !crate::cli::valid_studio_name(studio_name)
                || state.studio.is_none()
                || state.finished.is_some()
            {
                continue;
            }
            let sdir = store::studio_dir(&self.base, studio_name);
            for req in &state.outward_requests {
                if self.hstate.borrow().has_outward_card(studio_name, req.seq)
                    || state.has_outward_source_completed(req.seq)
                {
                    continue;
                }
                match build_proposal(studio_name, &sdir, state, req) {
                    Ok((action, why, base)) => {
                        let _ = std::fs::create_dir_all(hdir);
                        let event = NewHollywoodEvent::new(
                            EventBy::role("stage"),
                            HollywoodKind::CardProposed { action, why, base },
                        );
                        let _ = store::append_hollywood(hdir, |_, _| {
                            Ok::<_, std::convert::Infallible>(HPending {
                                event,
                                files: Vec::new(),
                            })
                        });
                        self.hrefresh(hdir);
                    }
                    Err(reason) => {
                        let completed = EventKind::OutwardCompleted {
                            card: 0,
                            source_seq: req.seq,
                            outcome: OutwardOutcome::Failed,
                            exit_code: None,
                            output_tail: reason,
                            log_file: None,
                        };
                        let _ = store::append(&sdir, |_, _| {
                            Ok::<_, std::convert::Infallible>(Pending {
                                event: NewEvent::new(EventBy::role("stage"), completed),
                                files: Vec::new(),
                            })
                        });
                    }
                }
            }
        }
        Ok(())
    }
}

fn build_proposal(
    studio: &str,
    sdir: &Path,
    state: &StudioState,
    req: &OutwardRequestRecord,
) -> Result<(CardAction, String, CardBase), String> {
    if req.argv.is_empty() {
        return Err("outward request has empty argv".into());
    }
    let cfg = state.studio.as_ref().ok_or("no studio config")?;
    let work_area = crate::workareas::role_dir(sdir, &cfg.repo, &req.role, cfg.no_git);
    let cwd = Path::new(&req.cwd);
    if !cwd.is_dir() {
        return Err(format!("outward cwd does not exist: {}", cwd.display()));
    }
    let norm_cwd = normalize_resolve(cwd, cwd);
    let norm_work = normalize_resolve(cwd, &work_area);
    if !path_inside(&norm_cwd, &[norm_work]) {
        return Err("outward cwd outside role work area".into());
    }
    let clean = stage_probe_env();
    let ctx = GitContext {
        policy: &cfg.push,
        cwd,
        role_dir: None,
        work_area: Some(&work_area),
        repo_root: Some(Path::new(&cfg.repo)),
        members: &cfg.members,
        env: &clean,
    };
    let rule = match classify_with_context(&req.argv[0], &req.argv[1..], "", &ctx).verdict {
        GuardVerdict::Card { rule, .. } => rule,
        GuardVerdict::Allow => return Err("outward command no longer needs a card".into()),
        GuardVerdict::Refuse { rule, .. } => {
            return Err(format!("outward command cannot run via a card ({rule})"));
        }
    };
    verify_local_git_clean(cwd, &clean)?;

    let (0, top_out, _) = run_real_git(&["rev-parse", "--show-toplevel"], cwd, &clean, None)
        .ok_or("git rev-parse --show-toplevel failed")?
    else {
        return Err("cwd is not inside a git repository".into());
    };
    let repo_top = top_out.trim().to_string();
    let (0, head_out, _) = run_real_git(&["rev-parse", "HEAD"], cwd, &clean, None)
        .ok_or("git rev-parse HEAD failed")?
    else {
        return Err("cannot resolve HEAD".into());
    };
    let head_oid = head_out.trim().to_string();

    let (remote_url, remote_ref, remote_oid, commits, commits_total) = if is_gh_pr_create(&req.argv)
    {
        inspect_gh_pr(&req.argv, cwd, &ctx, cfg, &head_oid)?
    } else if req.argv[0].rsplit('/').next() == Some("git") {
        inspect_git_push(&req.argv[1..], cwd, &ctx, &head_oid)?
    } else {
        return Err(format!("unsupported outward command '{}'", req.argv[0]));
    };

    let why = req
        .why
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(String::from)
        .or_else(|| req.rule.clone())
        .unwrap_or_else(|| rule.clone());
    let action = CardAction::Outward {
        studio: studio.into(),
        role: req.role.clone(),
        source_seq: req.seq,
        argv: req.argv.clone(),
        cwd: req.cwd.clone(),
        repo: repo_top.clone(),
        remote_url,
        rule,
        commits,
        commits_total,
    };
    let base = CardBase::GitHead {
        repo: repo_top,
        head_oid,
        remote_ref,
        remote_oid,
    };
    Ok((action, why, base))
}

/// Check git `HEAD` and `gh pr create` remote branch staleness at apply/execution time.
pub(crate) fn outward_stale(
    action: &CardAction,
    base: &CardBase,
    studio: Option<&StudioState>,
) -> Option<String> {
    let (
        CardAction::Outward {
            argv,
            cwd,
            remote_url,
            ..
        },
        CardBase::GitHead {
            head_oid,
            remote_ref,
            ..
        },
    ) = (action, base)
    else {
        return None;
    };
    let cwd_p = Path::new(cwd);
    let clean = stage_probe_env();
    let cur_head = run_real_git(&["rev-parse", "HEAD"], cwd_p, &clean, None)
        .and_then(|(c, out, _)| (c == 0).then(|| out.trim().to_string()))
        .unwrap_or_default();
    if cur_head != *head_oid {
        let was: String = head_oid.chars().take(7).collect();
        let now: String = cur_head.chars().take(7).collect();
        return Some(format!("HEAD moved (was {was}, now {now})"));
    }
    if is_gh_pr_create(argv)
        && let Some(st) = studio
        && let Some(cfg) = &st.studio
    {
        let branch = remote_ref.strip_prefix("refs/heads/").unwrap_or(remote_ref);
        let ctx = GitContext {
            policy: &cfg.push,
            cwd: cwd_p,
            role_dir: None,
            work_area: None,
            repo_root: Some(Path::new(&cfg.repo)),
            members: &cfg.members,
            env: &clean,
        };
        let remote_name = resolve_default_remote(cwd_p, Some(branch), &ctx);
        if let Err(e) = verify_branch_on_remote(
            &cfg.push,
            cwd_p,
            &clean,
            &remote_name,
            remote_url,
            branch,
            &cur_head,
        ) {
            return Some(e);
        }
    }
    None
}

/// Append detailed preview lines for an `Outward` card in `backlot card show`.
pub(crate) fn append_outward_preview(out: &mut String, card: &Card) {
    let CardAction::Outward {
        studio,
        role,
        argv,
        repo,
        remote_url,
        rule,
        commits,
        commits_total,
        ..
    } = &card.action
    else {
        return;
    };
    out.push_str(&format!(
        "\nstudio: {studio}\nrole: {role}\ncommand: {}\nrepo: {repo}\nremote: {remote_url}\nrule: {rule}",
        argv.join(" ")
    ));
    if commits.is_empty() && *commits_total == 0 {
        out.push_str("\ncommits: (none)");
    } else {
        out.push_str(&format!("\ncommits ({commits_total}):"));
        for line in commits.iter().take(20) {
            out.push_str(&format!("\n  {line}"));
        }
        if (*commits_total as usize) > commits.len() {
            out.push_str(&format!(
                "\n  … (+{} more)",
                (*commits_total as usize) - commits.len()
            ));
        }
    }
}
