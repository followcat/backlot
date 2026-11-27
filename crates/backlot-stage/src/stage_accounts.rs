//! Stage integration for implicit account tracking and `<state_dir>/accounts.json`.

use std::collections::{BTreeMap, BTreeSet};
use std::path::PathBuf;

use backlot_model::state::StudioState;
use backlot_model::summary::QuotaWindow;

use crate::accounts::{self, DerivedAccount};
use crate::accounts_store::{self, AccountsFile};
use crate::daemon::{Stage, load_studio};
use crate::handoff::HBlockedKind;
use crate::store;

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

    pub(crate) fn record_account_role(
        &self,
        studio: &str,
        role: &str,
        runner: &str,
        model: &str,
    ) {
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
        if self.cached.is_empty() && !self.base.as_os_str().is_empty()
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

        // Collect all studio roles + active rate-limited attentions.
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

        // Resolve env once per distinct runner while keeping multi-path disambiguation ready.
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
}
