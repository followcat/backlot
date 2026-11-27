//! Stage integration for implicit account tracking, `<state_dir>/accounts.json`,
//! and projecting shared `AccountSummary` views into `Summary` and CLI fallbacks.

use std::collections::{BTreeMap, BTreeSet};
use std::path::PathBuf;

use backlot_model::state::StudioState;
use backlot_model::summary::{
    AccountRoleSummary, AccountSummary, QuotaWindow, Summary, Work,
};

use crate::accounts::{self, DerivedAccount};
use crate::accounts_store::{self, AccountsFile};
use crate::daemon::{Stage, load_studio};
use crate::handoff::HBlockedKind;
use crate::store;

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
        let saved_active = saved.is_some_and(|e| {
            e.stalled && !e.reset_at_ms.is_some_and(|r| now_ms >= r)
        });
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
        let saved_stalled = saved.is_some_and(|e| {
            e.stalled && !e.reset_at_ms.is_some_and(|r| now_ms >= r)
        });
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

/// `(runner, model)` for any configured or started studio role, falling back to cast defaults.
pub(crate) fn studio_role_runner_model<'a>(
    state: &'a StudioState,
    role: &str,
) -> Option<(&'a str, &'a str)> {
    if let Some(rs) = state.roles.get(role)
        && let Some(actor) = rs.actor.as_ref()
    {
        return Some((actor.runner.as_str(), actor.model.as_str()));
    }
    let cfg = state.studio.as_ref()?;
    match role {
        "producer" => Some((cfg.roles.producer.runner.as_str(), cfg.roles.producer.model.as_str())),
        "director" => Some((cfg.roles.director.runner.as_str(), cfg.roles.director.model.as_str())),
        "writer" => Some((cfg.roles.writer.runner.as_str(), cfg.roles.writer.model.as_str())),
        _ => None,
    }
}

pub(crate) fn studio_role_model<'a>(state: &'a StudioState, role: &str) -> &'a str {
    studio_role_runner_model(state, role)
        .map(|(_, m)| m)
        .unwrap_or("")
}

impl Stage {
    pub(crate) fn effective_state_dir(&self) -> Option<PathBuf> {
        if !self.state_dir.is_empty() {
            Some(PathBuf::from(&self.state_dir))
        } else if !self.base.as_os_str().is_empty() {
            Some(self.base.clone())
        } else {
            None
        }
    }

    pub(crate) fn effective_host_env(&self) -> BTreeMap<String, String> {
        accounts::effective_host_env(&self.env_vars)
    }

    pub(crate) fn ensure_accounts_loaded(&self) {
        if self.accounts_loaded.get() {
            return;
        }
        self.accounts_loaded.set(true);
        let Some(dir) = self.effective_state_dir() else {
            return;
        };
        let (loaded, warn) = accounts_store::load(&dir);
        *self.accounts.borrow_mut() = loaded;
        if let Some(msg) = warn
            && self.hlogged.borrow_mut().insert(msg.clone())
        {
            crate::stagelog::stage_log!("{msg}");
        }
    }

    fn flush_accounts(&self) {
        let Some(dir) = self.effective_state_dir() else {
            return;
        };
        let snapshot = self.accounts.borrow().clone();
        if let Err(e) = accounts_store::save(&dir, &snapshot) {
            let msg = format!("accounts save: {e}");
            if self.hlogged.borrow_mut().insert(msg.clone()) {
                crate::stagelog::stage_log!("{msg}");
            }
        }
    }

    pub(crate) fn derive_role_account(&self, runner: &str, model: &str) -> DerivedAccount {
        let env = self.runner_env(runner).unwrap_or_default();
        let host = self.effective_host_env();
        accounts::derive_account(runner, model, &env, &host)
    }

    pub(crate) fn record_account_role(&self, studio: &str, role: &str, runner: &str, model: &str) {
        self.ensure_accounts_loaded();
        let acct = self.derive_role_account(runner, model);
        let changed = self
            .accounts
            .borrow_mut()
            .observe_role(&acct.id, &acct.runner, &acct.source, studio, role, model);
        if changed {
            self.flush_accounts();
        }
    }

    #[allow(clippy::too_many_arguments)]
    pub(crate) fn record_account_stall(
        &self,
        studio: &str,
        role: &str,
        runner: &str,
        model: &str,
        now_ms: u64,
        reset_at_ms: Option<u64>,
        window: QuotaWindow,
        detail: &str,
    ) {
        self.ensure_accounts_loaded();
        let acct = self.derive_role_account(runner, model);
        let rref = accounts_store::role_ref(studio, role);
        let changed = {
            let mut store = self.accounts.borrow_mut();
            let a = store.observe_role(&acct.id, &acct.runner, &acct.source, studio, role, model);
            let b = store.record_stall(
                &acct.id,
                &acct.runner,
                &acct.source,
                &rref,
                now_ms,
                reset_at_ms,
                window,
                detail,
            );
            a || b
        };
        if changed {
            self.flush_accounts();
        }
    }

    pub(crate) fn clear_account_stall_for_role(&self, studio: &str, role: &str) {
        let rref = accounts_store::role_ref(studio, role);
        self.ensure_accounts_loaded();
        if !self.accounts.borrow().has_stalled_role(&rref) {
            return;
        }
        let changed = self.accounts.borrow_mut().clear_role_stall(&rref);
        if changed {
            self.flush_accounts();
        }
    }

    /// Reconcile persistent `<state_dir>/accounts.json` against known studio roles,
    /// active `rate limited` attentions, Hollywood watch state, and clock expiry.
    pub(crate) fn sync_accounts_at(&mut self, now_ms: u64) {
        self.ensure_accounts_loaded();
        if self.cached.is_empty()
            && !self.base.as_os_str().is_empty()
            && let Ok(studios) = store::studios(&self.base)
        {
            for name in studios {
                let dir = store::studio_dir(&self.base, &name);
                if dir.join("events.jsonl").is_file()
                    && let Ok(state) = load_studio(&dir)
                {
                    self.cached.insert(name, state);
                }
            }
        }
        let mut changed = false;
        let mut active_stalled = BTreeSet::new();

        let mut role_observations: Vec<(String, String, String, String)> = Vec::new();
        for (studio, state) in &self.cached {
            for role in ["producer", "director", "writer"] {
                if let Some((runner, model)) = studio_role_runner_model(state, role) {
                    role_observations.push((
                        studio.clone(),
                        role.to_string(),
                        runner.to_string(),
                        model.to_string(),
                    ));
                }
            }
            for (role, rs) in &state.roles {
                if matches!(role.as_str(), "producer" | "director" | "writer") {
                    continue;
                }
                if let Some(actor) = rs.actor.as_ref() {
                    role_observations.push((
                        studio.clone(),
                        role.clone(),
                        actor.runner.clone(),
                        actor.model.clone(),
                    ));
                }
            }
            for attn in state.open_attention() {
                if attn.reason == crate::ratelimit::RATE_LIMITED {
                    active_stalled.insert(accounts_store::role_ref(studio, &attn.role));
                }
            }
        }
        if let Some(cfg) = self.hcfg.borrow().as_ref() {
            role_observations.push((
                "hollywood".into(),
                "hollywood".into(),
                cfg.runner.clone(),
                cfg.model.clone(),
            ));
        } else if let Some(actor) = self.hstate.borrow().role.actor.as_ref() {
            role_observations.push((
                "hollywood".into(),
                "hollywood".into(),
                actor.runner.clone(),
                actor.model.clone(),
            ));
        }

        let host = self.effective_host_env();
        let mut env_by_runner: BTreeMap<String, BTreeMap<String, String>> = BTreeMap::new();
        for (_, _, runner, _) in &role_observations {
            env_by_runner
                .entry(runner.clone())
                .or_insert_with(|| self.runner_env(runner).unwrap_or_default());
        }
        let derived = {
            let inputs: Vec<(&str, &str, &BTreeMap<String, String>)> = role_observations
                .iter()
                .map(|(_, _, runner, model)| {
                    (
                        runner.as_str(),
                        model.as_str(),
                        &env_by_runner[runner.as_str()],
                    )
                })
                .collect();
            accounts::resolve_accounts(&inputs, &host)
        };

        let hollywood_rate_limited = if self
            .work_looked
            .borrow()
            .contains_key(&crate::hstage::hkey())
        {
            Some(matches!(
                *self.herror.borrow(),
                Some(HBlockedKind::RateLimited { .. } | HBlockedKind::OutOfQuota { .. })
            ))
        } else {
            None
        };

        {
            let mut store: std::cell::RefMut<'_, AccountsFile> = self.accounts.borrow_mut();
            for ((studio, role, _runner, model), acct) in
                role_observations.iter().zip(derived.iter())
            {
                if store.observe_role(&acct.id, &acct.runner, &acct.source, studio, role, model) {
                    changed = true;
                }
            }
            if store.reconcile(now_ms, &active_stalled, hollywood_rate_limited) {
                changed = true;
            }
        }
        if changed {
            self.flush_accounts();
        }
    }

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
            runner_envs.insert(
                runner.clone(),
                self.runner_env(runner).unwrap_or_default(),
            );
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
