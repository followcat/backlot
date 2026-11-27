//! A studio's cast after creation (`studio.cast`, "Changing a studio
//! after it starts"): the settings `backlot studio cast set` records —
//! the three roles, the candidates, the limits, the checks and the
//! timings. The cast file given to `studio new` initializes a studio;
//! from then on this is how it changes.
//!
//! The studio's binding — `repo`, `members`, `no_git`, `mode` — is not
//! cast and never travels here. The push policy travels as its three
//! knobs; the remote URLs are facts recorded from the bound repository
//! at creation, and the reducer keeps them.
//!
//! [`check`] is the runnable half of the cast file's rules: the studio
//! exists and is not finished, the settings change something, a limit
//! is a positive number, every identity's effort, cline model and
//! permission profile hold, and `[limits]` can still seat the fixed
//! roles. Whether a runner has an adapter and is installed is the
//! stage's own check, so it stays there.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use crate::event::{
    ActorIdentity, Candidate, CheckDecl, DesignGate, EventKind, PushPolicy, RoleConfigs,
    default_escalate_after_s, default_max_actors, default_nudge_after_s, default_rework_cap,
    default_stall_limit_s,
};
use crate::refusal::Refusal;
use crate::state::StudioState;

// The printed forms live beside this file; the `cast::` path carries
// them all.
pub use crate::cast_diff::{
    CandidateRow, CastDiff, CheckRow, IdentityRow, LimitRow, ValueRow, diff,
};
pub use crate::cast_file::to_toml;

/// The cast fields a studio's log carries after `studio.created` —
/// the full new settings a `studio.cast` records.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CastSettings {
    pub roles: RoleConfigs,
    #[serde(default)]
    pub candidates: Vec<Candidate>,
    #[serde(default = "default_rework_cap")]
    pub rework_cap: u32,
    #[serde(default = "default_stall_limit_s")]
    pub stall_limit_s: u32,
    #[serde(default = "default_nudge_after_s")]
    pub nudge_after_s: u32,
    #[serde(default = "default_escalate_after_s")]
    pub escalate_after_s: u32,
    #[serde(default)]
    pub check_env: Vec<String>,
    #[serde(default)]
    pub checks: Vec<CheckDecl>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub task_budget_s: Option<u32>,
    #[serde(default)]
    pub limits: BTreeMap<String, u32>,
    /// At most this many actors live at once; fixed roles never count.
    #[serde(default = "default_max_actors")]
    pub max_actors: u32,
    /// The push policy's knobs; `remotes` is ignored on the way in and
    /// kept from the recorded settings on the way out.
    #[serde(default)]
    pub push: PushPolicy,
    /// Opening design gate policy (`required` or `off`).
    #[serde(default, skip_serializing_if = "DesignGate::is_off")]
    pub design_gate: DesignGate,
}

impl CastSettings {
    /// The cast the studio plays now — the fixed roles as their newest
    /// `role.recast` has them, the recorded cast otherwise. This is
    /// what `backlot studio cast show` prints, what a diff compares
    /// against, and what "nothing changes" means.
    pub fn of(state: &StudioState) -> Option<Self> {
        let cfg = state.studio.as_ref()?;
        Some(CastSettings {
            roles: RoleConfigs {
                producer: state.fixed_identity("producer")?.clone(),
                director: state.fixed_identity("director")?.clone(),
                writer: state.fixed_identity("writer")?.clone(),
            },
            candidates: cfg.candidates.clone(),
            rework_cap: cfg.rework_cap,
            stall_limit_s: cfg.stall_limit_s,
            nudge_after_s: cfg.nudge_after_s,
            escalate_after_s: cfg.escalate_after_s,
            check_env: cfg.check_env.clone(),
            checks: cfg.checks.clone(),
            task_budget_s: cfg.task_budget_s,
            limits: cfg.limits.clone(),
            max_actors: cfg.max_actors,
            push: cfg.push.clone(),
            design_gate: cfg.design_gate,
        })
    }

    /// The three roles, then every candidate — every identity the
    /// cast file's effort, cline-model and profile rules apply to.
    pub fn identities(&self) -> impl Iterator<Item = ActorIdentity> + '_ {
        [
            self.roles.producer.clone(),
            self.roles.director.clone(),
            self.roles.writer.clone(),
        ]
        .into_iter()
        .chain(self.candidates.iter().map(|c| c.actor()))
    }

    /// Whether two settings say the same cast. The recorded remote URLs
    /// are facts, not settings, so they never make a cast "different".
    pub fn same_cast(&self, other: &CastSettings) -> bool {
        self.roles == other.roles
            && self.candidates == other.candidates
            && self.rework_cap == other.rework_cap
            && self.stall_limit_s == other.stall_limit_s
            && self.nudge_after_s == other.nudge_after_s
            && self.escalate_after_s == other.escalate_after_s
            && self.check_env == other.check_env
            && self.checks == other.checks
            && self.task_budget_s == other.task_budget_s
            && self.limits == other.limits
            && self.max_actors == other.max_actors
            && self.push.protected_branches == other.push.protected_branches
            && self.push.protect_tags == other.push.protect_tags
            && self.push.protect_default_branch == other.push.protect_default_branch
            && self.design_gate == other.design_gate
    }

    /// The `studio.cast` event that records these settings, with the
    /// card marker a card's execution carries.
    pub fn event_kind(&self, card: Option<String>) -> EventKind {
        EventKind::StudioCast {
            roles: Box::new(self.roles.clone()),
            candidates: self.candidates.clone(),
            rework_cap: self.rework_cap,
            stall_limit_s: self.stall_limit_s,
            nudge_after_s: self.nudge_after_s,
            escalate_after_s: self.escalate_after_s,
            check_env: self.check_env.clone(),
            checks: self.checks.clone(),
            task_budget_s: self.task_budget_s,
            limits: self.limits.clone(),
            max_actors: self.max_actors,
            push: self.push.clone(),
            design_gate: self.design_gate,
            card,
        }
    }
}

/// The runnable cast rules, at the append boundary and in the CLI and a
/// card's proposal alike: the studio exists and is not finished, the new
/// settings change something, every limit is positive, every identity
/// holds the effort / cline-model / permission-profile rules, and
/// `[limits]` can still seat the fixed roles.
pub fn check(state: &StudioState, new: &CastSettings) -> Result<(), Refusal> {
    // The effective cast, not the recorded one: a role swapped by
    // `role.recast` already plays the new identity, so a file that
    // spells it does change nothing.
    if CastSettings::of(state).is_none_or(|old| old.same_cast(new)) {
        if state.studio.is_none() {
            return Err(Refusal::NoStudio);
        }
        if state.finished.is_some() {
            return Err(Refusal::AlreadyFinished);
        }
        return Err(Refusal::CastUnchanged);
    }
    check_runnable(state, new)
}

/// The runnable rules without the "nothing changes" comparison — what
/// the append boundary applies to the `studio.cast` event. A cast change
/// is recorded as its recasts first (each named by the file), then the
/// settings event; by the time the settings land, the effective cast
/// already carries the moved identities, so comparing them again would
/// refuse the very event that records the file. The CLI and a card's
/// proposal run [`check`], which is where "nothing changes" is said.
pub fn check_runnable(state: &StudioState, new: &CastSettings) -> Result<(), Refusal> {
    if state.studio.is_none() {
        return Err(Refusal::NoStudio);
    }
    if state.finished.is_some() {
        return Err(Refusal::AlreadyFinished);
    }
    for (runner, cap) in &new.limits {
        if *cap == 0 {
            return Err(Refusal::CastBadLimit {
                runner: runner.clone(),
            });
        }
    }
    // A studio with no actor slot can never cast: the cap is a real
    // number like every other cast value.
    if new.max_actors < 1 {
        return Err(Refusal::CastBadMaxActors);
    }
    for actor in new.identities() {
        if !matches!(
            actor.permission_profile.as_str(),
            "standard" | "worktree" | "readonly"
        ) {
            return Err(Refusal::ProfileNotSupported(
                actor.permission_profile.clone(),
            ));
        }
        crate::recast::check_effort(&actor)?;
        crate::recast::check_model(&actor)?;
    }
    crate::recast::seats(
        &new.roles.producer,
        &new.roles.director,
        &new.roles.writer,
        &new.candidates,
        &new.limits,
    )
}
