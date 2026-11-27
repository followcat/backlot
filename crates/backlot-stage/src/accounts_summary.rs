//! Shared `AccountSummary` projection for `Stage::summary_view_at` and CLI fallbacks.

use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

use backlot_model::hollywood::HollywoodState;
use backlot_model::state::StudioState;
use backlot_model::summary::{AccountRoleSummary, AccountSummary, Summary, Work};

use crate::accounts;
use crate::accounts_store::{self, AccountsFile};
use crate::config::HollywoodCfg;
use crate::daemon::Stage;
use crate::handoff::HBlockedKind;
use crate::live::LiveView;
use crate::stage_accounts::studio_role_runner_model;

/// Input describing one studio or Hollywood role for shared account rollup.
pub(crate) struct RoleAccountInput {
    pub studio: String,
    pub role: String,
    pub runner: String,
    pub model: String,
    pub active: bool,
    pub working: bool,
    pub held: bool,
    pub stalled: bool,
    pub fallback_reset_ms: Option<u64>,
    pub fallback_stall_ms: Option<u64>,
}

/// Build sorted [`AccountSummary`] entries and per-role account assignments from
/// role inputs, resolved runner environments, and persisted `<state_dir>/accounts.json`.
pub(crate) fn build_account_summaries(
    roles: &[RoleAccountInput],
    runner_envs: &BTreeMap<String, BTreeMap<String, String>>,
    host_env: &BTreeMap<String, String>,
    persisted: &AccountsFile,
    now_ms: u64,
) -> (Vec<AccountSummary>, BTreeMap<(String, String), String>) {
    let empty_env = BTreeMap::new();
    let inputs: Vec<(&str, &str, &BTreeMap<String, String>)> = roles
        .iter()
        .map(|r| {
            (
                r.runner.as_str(),
                r.model.as_str(),
                runner_envs.get(&r.runner).unwrap_or(&empty_env),
            )
        })
        .collect();
    let derived = accounts::resolve_accounts(&inputs, host_env);

    struct Bucket {
        runner: String,
        source: String,
        active: u32,
        working: u32,
        any_role_stalled: bool,
        fallback_stall_ms: Option<u64>,
        fallback_reset_ms: Option<u64>,
        roles: Vec<AccountRoleSummary>,
    }

    let mut buckets: BTreeMap<String, Bucket> = BTreeMap::new();
    let mut role_account_map: BTreeMap<(String, String), String> = BTreeMap::new();

    for (role, acct) in roles.iter().zip(derived.iter()) {
        role_account_map.insert((role.studio.clone(), role.role.clone()), acct.id.clone());
        let saved = persisted.accounts.get(&acct.id);
        let saved_active =
            saved.is_some_and(|e| e.stalled && !e.reset_at_ms.is_some_and(|r| now_ms >= r));
        let rref = accounts_store::role_ref(&role.studio, &role.role);
        let role_stalled = role.stalled
            || (saved_active && saved.is_some_and(|e| e.stalled_by.iter().any(|k| k == &rref)));

        let bucket = buckets.entry(acct.id.clone()).or_insert_with(|| Bucket {
            runner: acct.runner.clone(),
            source: acct.source.clone(),
            active: 0,
            working: 0,
            any_role_stalled: false,
            fallback_stall_ms: None,
            fallback_reset_ms: None,
            roles: Vec::new(),
        });
        bucket.active += u32::from(role.active);
        bucket.working += u32::from(role.working);
        bucket.any_role_stalled |= role_stalled;
        if let Some(ts) = role.fallback_stall_ms {
            bucket.fallback_stall_ms = Some(bucket.fallback_stall_ms.map_or(ts, |p| p.max(ts)));
        }
        if let Some(reset) = role.fallback_reset_ms {
            bucket.fallback_reset_ms = Some(reset);
        }
        bucket.roles.push(AccountRoleSummary {
            studio: role.studio.clone(),
            role: role.role.clone(),
            model: role.model.clone(),
            active: role.active,
            working: role.working,
            held: role.held,
            stalled: role_stalled,
        });
    }

    let mut out = Vec::with_capacity(buckets.len());
    for (id, mut bucket) in buckets {
        bucket.roles.sort_by(|a, b| {
            let a_hw = a.studio != "hollywood";
            let b_hw = b.studio != "hollywood";
            a_hw.cmp(&b_hw)
                .then_with(|| a.studio.cmp(&b.studio))
                .then_with(|| a.role.cmp(&b.role))
        });
        let saved = persisted.accounts.get(&id);
        let saved_stalled =
            saved.is_some_and(|e| e.stalled && !e.reset_at_ms.is_some_and(|r| now_ms >= r));
        let stalled = bucket.any_role_stalled || saved_stalled;
        let last_stall_ms = saved
            .and_then(|e| e.last_stall_ms)
            .or(bucket.fallback_stall_ms);
        let reset_at_ms = saved
            .and_then(|e| e.reset_at_ms)
            .or(bucket.fallback_reset_ms);
        let window = saved.and_then(|e| e.window);
        out.push(AccountSummary {
            id,
            runner: bucket.runner,
            source: bucket.source,
            active: bucket.active,
            working: bucket.working,
            max_active: None,
            room: None,
            queued: 0,
            stalled,
            last_stall_ms,
            reset_at_ms,
            window,
            roles: bucket.roles,
        });
    }
    (out, role_account_map)
}

/// CLI fallback: derive lot-wide account summaries from ledgers, config, `<base>/accounts.json`,
/// and any fresh `LiveView` work samples when `live.json` is absent or predates `accounts`.
pub(crate) fn derive_accounts_for_lot(
    base: &Path,
    env_vars: &BTreeMap<String, String>,
    studios: &[(String, StudioState)],
    hstate: &HollywoodState,
    hcfg: Option<&HollywoodCfg>,
    live: &LiveView,
    now_ms: u64,
) -> Vec<AccountSummary> {
    let (persisted, _) = accounts_store::load(base);
    let mut inputs = Vec::new();
    let mut runners = BTreeSet::new();

    for (studio_id, state) in studios {
        if state.finished.is_some() {
            continue;
        }
        let wanted = backlot_model::wanted::wanted_roles(state);
        let mut seen_roles = BTreeSet::new();

        for role in ["producer", "director", "writer"] {
            seen_roles.insert(role.to_string());
            let Some((runner, model)) = studio_role_runner_model(state, role) else {
                continue;
            };
            push_studio_role_input(
                &mut inputs,
                &mut runners,
                studio_id,
                role,
                runner,
                model,
                state,
                live,
            );
        }
        for (role, rs) in &state.roles {
            if seen_roles.contains(role) {
                continue;
            }
            if rs.closed {
                continue;
            }
            let Some(actor) = rs.actor.as_ref() else {
                continue;
            };
            seen_roles.insert(role.clone());
            push_studio_role_input(
                &mut inputs,
                &mut runners,
                studio_id,
                role,
                &actor.runner,
                &actor.model,
                state,
                live,
            );
        }
        for w in &wanted {
            if seen_roles.contains(&w.role) {
                continue;
            }
            seen_roles.insert(w.role.clone());
            push_studio_role_input(
                &mut inputs,
                &mut runners,
                studio_id,
                &w.role,
                &w.actor.runner,
                &w.actor.model,
                state,
                live,
            );
        }
    }

    if let Some((runner, model)) =
        hcfg.map(|c| (c.runner.as_str(), c.model.as_str()))
            .or_else(|| {
                hstate
                    .role
                    .actor
                    .as_ref()
                    .map(|a| (a.runner.as_str(), a.model.as_str()))
            })
    {
        runners.insert(runner.to_string());
        let active = hstate.role.live && !hstate.role.closed;
        let working = active
            && live
                .work
                .get(&("hollywood".into(), "hollywood".into()))
                .copied()
                == Some(Work::Working);
        inputs.push(RoleAccountInput {
            studio: "hollywood".into(),
            role: "hollywood".into(),
            runner: runner.to_string(),
            model: model.to_string(),
            active,
            working,
            held: false,
            stalled: false,
            fallback_stall_ms: None,
            fallback_reset_ms: None,
        });
    }

    let host = accounts::effective_host_env(env_vars);
    let runner_envs = accounts::runner_envs_for_state_dir(base, &host, &runners);
    let (summaries, _) = build_account_summaries(&inputs, &runner_envs, &host, &persisted, now_ms);
    summaries
}

#[allow(clippy::too_many_arguments)]
fn push_studio_role_input(
    inputs: &mut Vec<RoleAccountInput>,
    runners: &mut BTreeSet<String>,
    studio_id: &str,
    role: &str,
    runner: &str,
    model: &str,
    state: &StudioState,
    live: &LiveView,
) {
    runners.insert(runner.to_string());
    let active = state.roles.get(role).is_some_and(|r| r.live);
    let working = active
        && live
            .work
            .get(&(studio_id.to_string(), role.to_string()))
            .copied()
            == Some(Work::Working);
    let open_rl = state
        .open_attention()
        .find(|a| a.role == role && a.reason == crate::ratelimit::RATE_LIMITED);
    inputs.push(RoleAccountInput {
        studio: studio_id.to_string(),
        role: role.to_string(),
        runner: runner.to_string(),
        model: model.to_string(),
        active,
        working,
        held: false,
        stalled: open_rl.is_some(),
        fallback_stall_ms: open_rl.map(|a| a.ts_ms),
        fallback_reset_ms: open_rl.and_then(|a| a.reset_at_ms),
    });
}

impl Stage {
    /// Populate `RoleSummary.account`, `HollywoodSummary.account`, and aggregated
    /// `Summary.accounts` for `backlot/1` and `live.json`.
    pub(crate) fn attach_accounts_to_summary(&self, s: &mut Summary, now_ms: u64) {
        self.ensure_accounts_loaded();
        let mut inputs = Vec::new();
        let mut runners = BTreeSet::new();

        for studio in &s.studios {
            let studio_state = self.cached.get(&studio.id);
            let is_open_studio = studio.state != "finished";
            for role in &studio.roles {
                let Some(runner) = role.runner.as_deref() else {
                    continue;
                };
                if !is_open_studio {
                    continue;
                }
                let is_fixed = matches!(role.role.as_str(), "producer" | "director" | "writer");
                if role.state == "closed" && !is_fixed {
                    continue;
                }
                runners.insert(runner.to_string());
                let open_rl = studio_state.and_then(|st| {
                    st.open_attention()
                        .find(|a| a.role == role.role && a.reason == crate::ratelimit::RATE_LIMITED)
                });
                inputs.push(RoleAccountInput {
                    studio: studio.id.clone(),
                    role: role.role.clone(),
                    runner: runner.to_string(),
                    model: role.model.clone().unwrap_or_default(),
                    active: role.state == "live",
                    working: role.state == "live" && role.work == Some(Work::Working),
                    held: false,
                    stalled: open_rl.is_some(),
                    fallback_stall_ms: open_rl.map(|a| a.ts_ms),
                    fallback_reset_ms: open_rl.and_then(|a| a.reset_at_ms),
                });
            }
        }

        if let Some(hw) = s.hollywood.as_ref()
            && let Some(runner) = hw.runner.as_deref()
        {
            runners.insert(runner.to_string());
            let hw_stalled = matches!(
                *self.herror.borrow(),
                Some(HBlockedKind::RateLimited { .. } | HBlockedKind::OutOfQuota { .. })
            );
            inputs.push(RoleAccountInput {
                studio: "hollywood".into(),
                role: "hollywood".into(),
                runner: runner.to_string(),
                model: hw.model.clone().unwrap_or_default(),
                active: hw.state == "live",
                working: hw.state == "live" && hw.work == Some(Work::Working),
                held: false,
                stalled: hw_stalled,
                fallback_stall_ms: None,
                fallback_reset_ms: None,
            });
        }

        let host = self.effective_host_env();
        let mut runner_envs = BTreeMap::new();
        for runner in &runners {
            runner_envs.insert(runner.clone(), self.runner_env(runner).unwrap_or_default());
        }

        let persisted = self.accounts.borrow();
        let (summaries, role_map) =
            build_account_summaries(&inputs, &runner_envs, &host, &persisted, now_ms);

        for studio in &mut s.studios {
            for role in &mut studio.roles {
                if let Some(id) = role_map.get(&(studio.id.clone(), role.role.clone())) {
                    role.account = Some(id.clone());
                } else if let Some(runner) = role.runner.as_deref() {
                    let model = role.model.as_deref().unwrap_or("");
                    let env = runner_envs
                        .get(runner)
                        .cloned()
                        .unwrap_or_else(|| self.runner_env(runner).unwrap_or_default());
                    role.account = Some(accounts::derive_account(runner, model, &env, &host).id);
                }
            }
        }
        if let Some(hw) = s.hollywood.as_mut()
            && let Some(id) = role_map.get(&("hollywood".into(), "hollywood".into()))
        {
            hw.account = Some(id.clone());
        }
        s.accounts = summaries;
    }
}
