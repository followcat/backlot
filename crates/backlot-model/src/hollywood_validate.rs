//! The append-time rules for Hollywood's ledger (D17a, R26): who
//! may write which kind, what a card's action/base pair must look
//! like, and where a card stands. Replay never runs these — the log
//! is always what it is.

use std::fmt;

use crate::hollywood::{
    CardAction, CardBase, HOLLYWOOD, HollywoodKind, NewHollywoodEvent, status_str,
};
use crate::hollywood_state::{CardState, HollywoodState};
use crate::state::TaskStatus;

/// Why a Hollywood append was refused — the append-time rules of
/// D17a/R26. Replay never sees it.
#[derive(Debug, Clone, PartialEq)]
pub enum HRefusal {
    /// The card id names nothing.
    NoCard(u64),
    /// The card is not open (apply, cancel, stale).
    CardClosed {
        card: u64,
        state: &'static str,
    },
    /// `card.done`/`card.failed` on a card not `applied`.
    CardNotApplied {
        card: u64,
        state: &'static str,
    },
    /// `card.withdrawn` on a card someone else proposed — Hollywood
    /// may take back only its own.
    NotOwnCard(u64),
    /// `role '<r>' cannot do this; <who> can`.
    RoleBound {
        role: String,
        who: &'static str,
    },
    /// A card's action/base pair cannot stand (wrong base kind, or a
    /// status transition the studio's own rules already refuse).
    BadCard(String),
    /// An empty headline or a message whose shape does not fit.
    BadReport(String),
    BadMessage(String),
}

impl fmt::Display for HRefusal {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            HRefusal::NoCard(c) => write!(f, "no card 'c{c}'"),
            HRefusal::CardClosed { card, state } => {
                write!(f, "card 'c{card}' is already {state}")
            }
            HRefusal::CardNotApplied { card, state } => {
                write!(f, "card 'c{card}' is {state}, not applied")
            }
            HRefusal::NotOwnCard(card) => {
                write!(f, "card 'c{card}' was not proposed by hollywood")
            }
            HRefusal::RoleBound { role, who } => {
                write!(f, "role '{role}' cannot do this; {who} can")
            }
            HRefusal::BadCard(d) | HRefusal::BadReport(d) | HRefusal::BadMessage(d) => {
                f.write_str(d)
            }
        }
    }
}

impl std::error::Error for HRefusal {}

fn role_bound(role: &str, who: &'static str) -> HRefusal {
    HRefusal::RoleBound {
        role: role.into(),
        who,
    }
}

fn open_card(state: &HollywoodState, card: u64) -> Result<(), HRefusal> {
    match state.cards.get(&card) {
        None => Err(HRefusal::NoCard(card)),
        Some(c) if c.state != CardState::Open => Err(HRefusal::CardClosed {
            card,
            state: c.state.as_str(),
        }),
        Some(_) => Ok(()),
    }
}

/// Does the action/base pair stand — right base kind, and a status
/// the studio's own transition rules accept as the `was`.
fn base_fits(action: &CardAction, base: &CardBase) -> Result<(), HRefusal> {
    let bad = || {
        HRefusal::BadCard(format!(
            "base '{}' does not match action '{}'",
            base_kind(base),
            action_kind(action)
        ))
    };
    match (action, base) {
        (CardAction::StudioNew { .. }, CardBase::StudioAbsent) => Ok(()),
        (CardAction::Close { .. }, CardBase::PersonHeld) => Ok(()),
        (CardAction::Reopen { .. }, CardBase::PersonClosed) => Ok(()),
        (CardAction::Priority { .. }, CardBase::TaskPriority { .. }) => Ok(()),
        (CardAction::Pause { .. }, CardBase::TaskStatus { was }) => {
            // `task pause` lands only on a running task.
            if *was == TaskStatus::Running {
                Ok(())
            } else {
                Err(HRefusal::BadCard(format!(
                    "a pause base must be running, not {}",
                    status_str(*was)
                )))
            }
        }
        (CardAction::Resume { .. }, CardBase::TaskStatus { was }) => {
            if matches!(*was, TaskStatus::Paused | TaskStatus::Blocked) {
                Ok(())
            } else {
                Err(HRefusal::BadCard(format!(
                    "a resume base must be paused or blocked, not {}",
                    status_str(*was)
                )))
            }
        }
        (CardAction::Abandon { .. }, CardBase::TaskStatus { was }) => {
            if was.is_terminal() {
                Err(HRefusal::BadCard(format!(
                    "an abandon base cannot be {}",
                    status_str(*was)
                )))
            } else {
                Ok(())
            }
        }
        (CardAction::Recast { .. }, CardBase::RoleIdentity { .. }) => Ok(()),
        (CardAction::Restart { .. }, CardBase::PersonRestartable) => Ok(()),
        (CardAction::Cast { .. }, CardBase::CastSeq { .. }) => Ok(()),
        (CardAction::ReopenStudio { .. }, CardBase::StudioFinished) => Ok(()),
        (CardAction::Setup { .. }, CardBase::HollywoodIdentity { .. }) => Ok(()),
        (
            CardAction::Outward {
                studio,
                role,
                argv,
                repo,
                ..
            },
            CardBase::GitHead {
                repo: base_repo, ..
            },
        ) => {
            if studio.trim().is_empty()
                || role.trim().is_empty()
                || argv.is_empty()
                || repo != base_repo
            {
                Err(HRefusal::BadCard("invalid outward card base".into()))
            } else {
                Ok(())
            }
        }
        (
            CardAction::RepoNote {
                repo,
                entry_id,
                text,
                ..
            },
            CardBase::RepoNoteEntry {
                repo: base_repo,
                entry_id: base_id,
                ..
            },
        ) => {
            if repo.trim().is_empty() || repo != base_repo || entry_id != base_id {
                return Err(HRefusal::BadCard("invalid repo-note card base".into()));
            }
            if !crate::repo_notes::valid_entry_id(entry_id) {
                return Err(HRefusal::BadCard(format!(
                    "invalid repo-note id '{entry_id}' (expected [a-z0-9_-]{{1,40}})"
                )));
            }
            crate::repo_notes::check_text_safe(text).map_err(HRefusal::BadCard)
        }
        _ => Err(bad()),
    }
}

fn base_kind(base: &CardBase) -> &'static str {
    match base {
        CardBase::StudioAbsent => "studio_absent",
        CardBase::PersonHeld => "person_held",
        CardBase::PersonClosed => "person_closed",
        CardBase::TaskPriority { .. } => "task_priority",
        CardBase::TaskStatus { .. } => "task_status",
        CardBase::RoleIdentity { .. } => "role_identity",
        CardBase::PersonRestartable => "person_restartable",
        CardBase::CastSeq { .. } => "cast_seq",
        CardBase::StudioFinished => "studio_finished",
        CardBase::HollywoodIdentity { .. } => "hollywood_identity",
        CardBase::GitHead { .. } => "git_head",
        CardBase::RepoNoteEntry { .. } => "repo_note_entry",
    }
}

fn action_kind(action: &CardAction) -> &'static str {
    match action {
        CardAction::StudioNew { .. } => "studio_new",
        CardAction::Close { .. } => "close",
        CardAction::Reopen { .. } => "reopen",
        CardAction::Priority { .. } => "priority",
        CardAction::Pause { .. } => "pause",
        CardAction::Resume { .. } => "resume",
        CardAction::Abandon { .. } => "abandon",
        CardAction::Recast { .. } => "recast",
        CardAction::Restart { .. } => "restart",
        CardAction::Cast { .. } => "cast",
        CardAction::ReopenStudio { .. } => "reopen_studio",
        CardAction::Setup { .. } => "setup",
        CardAction::Outward { .. } => "outward",
        CardAction::RepoNote { .. } => "repo_note",
    }
}

/// The append-time rules for Hollywood's ledger. The role rules of
/// D17a/R26 run here so every append path is covered; replay stays
/// permissive.
pub fn validate(state: &HollywoodState, new: &NewHollywoodEvent) -> Result<(), HRefusal> {
    let role = new.by.role.as_str();
    match &new.kind {
        HollywoodKind::CardProposed { action, why, base } => {
            let allowed = if matches!(
                action,
                CardAction::Outward { .. } | CardAction::RepoNote { .. }
            ) {
                role == HOLLYWOOD || role == "stage"
            } else {
                role == HOLLYWOOD
            };
            if !allowed {
                return Err(role_bound(
                    role,
                    if matches!(
                        action,
                        CardAction::Outward { .. } | CardAction::RepoNote { .. }
                    ) {
                        "hollywood or the stage"
                    } else {
                        "hollywood"
                    },
                ));
            }
            if why.trim().is_empty() {
                return Err(HRefusal::BadCard("a card's why is empty".into()));
            }
            base_fits(action, base)
        }
        HollywoodKind::CardApplied { card } | HollywoodKind::CardCancelled { card, .. } => {
            if role != "user" {
                return Err(role_bound(role, "the user"));
            }
            open_card(state, *card)
        }
        // Hollywood may take back only a card it proposed itself, and
        // only while it is still open — an applied card is the stage's
        // to execute, and the user's to cancel.
        HollywoodKind::CardWithdrawn { card, .. } => {
            if role != HOLLYWOOD {
                return Err(role_bound(role, "hollywood"));
            }
            let Some(c) = state.cards.get(card) else {
                return Err(HRefusal::NoCard(*card));
            };
            if c.state != CardState::Open {
                return Err(HRefusal::CardClosed {
                    card: *card,
                    state: c.state.as_str(),
                });
            }
            if c.by != HOLLYWOOD {
                return Err(HRefusal::NotOwnCard(*card));
            }
            Ok(())
        }
        // A supersede is the ledger's own word for two proposals on one
        // fact — appended with the newer proposal, by whoever proposed
        // it, so the older can never be applied beside it.
        HollywoodKind::CardSuperseded { card, by_card, .. } => {
            if role != HOLLYWOOD && role != "stage" {
                return Err(role_bound(role, "hollywood or the stage"));
            }
            if !state.cards.contains_key(card) || !state.cards.contains_key(by_card) {
                return Err(HRefusal::NoCard(if !state.cards.contains_key(card) {
                    *card
                } else {
                    *by_card
                }));
            }
            open_card(state, *card)
        }
        HollywoodKind::CardDone { card, .. } | HollywoodKind::CardFailed { card, .. } => {
            if role != "stage" {
                return Err(role_bound(role, "the stage"));
            }
            if let HollywoodKind::CardFailed { reason, .. } = &new.kind
                && reason.trim().is_empty()
            {
                return Err(HRefusal::BadCard("card.failed needs a reason".into()));
            }
            match state.cards.get(card) {
                None => Err(HRefusal::NoCard(*card)),
                Some(c) if c.state != CardState::Applied => Err(HRefusal::CardNotApplied {
                    card: *card,
                    state: c.state.as_str(),
                }),
                Some(_) => Ok(()),
            }
        }
        HollywoodKind::CardStale { card, .. } => {
            // The user's apply-time check records on an open card;
            // the stage's execution-time check on an applied one
            // (H3) — a base may keep moving between the two.
            if role != "user" && role != "stage" {
                return Err(role_bound(role, "the user or the stage"));
            }
            match state.cards.get(card) {
                None => Err(HRefusal::NoCard(*card)),
                Some(c) if role == "stage" && c.state == CardState::Applied => Ok(()),
                Some(c) if role == "user" && c.state == CardState::Open => Ok(()),
                Some(c) => Err(HRefusal::CardClosed {
                    card: *card,
                    state: c.state.as_str(),
                }),
            }
        }
        HollywoodKind::Reported { headline, .. } => {
            if role != HOLLYWOOD {
                return Err(role_bound(role, "hollywood"));
            }
            if headline.trim().is_empty() || headline.contains('\n') {
                return Err(HRefusal::BadReport(
                    "a headline is one nonempty line".into(),
                ));
            }
            Ok(())
        }
        HollywoodKind::MessageSent {
            to, studio, file, ..
        } => match (role, to.as_str()) {
            ("user", HOLLYWOOD) if studio.is_none() => Ok(()),
            (HOLLYWOOD, "user") if studio.is_none() => Ok(()),
            (HOLLYWOOD, "producer") if studio.is_some() => Ok(()),
            // The stage's notices to Hollywood always carry a body
            // file under `messages/` (H3's notice path).
            ("stage", HOLLYWOOD) if file.is_some() => Ok(()),
            _ => Err(HRefusal::BadMessage(format!(
                "'{role}' may not send to '{to}'"
            ))),
        },
        // Hollywood's own lifecycle — the stage's word, the way a
        // studio's `role.*` events are.
        HollywoodKind::RoleState { .. } => {
            if role != "stage" {
                return Err(role_bound(role, "the stage"));
            }
            Ok(())
        }
        HollywoodKind::PermitGranted {
            id,
            scope,
            role: permit_role,
            rule,
            ..
        } => {
            if role != "user" {
                return Err(role_bound(role, "the user"));
            }
            if id.trim().is_empty() {
                return Err(HRefusal::BadCard("a permit id is empty".into()));
            }
            if state.permits.contains_key(id) {
                return Err(HRefusal::BadCard(format!("permit '{id}' already exists")));
            }
            if *scope != crate::event::PermitScope::Role
                || permit_role.as_deref() != Some(HOLLYWOOD)
            {
                return Err(HRefusal::BadCard(
                    "Hollywood permits are role-scoped to hollywood".into(),
                ));
            }
            if rule.trim().is_empty() {
                return Err(HRefusal::BadCard("a permit rule is empty".into()));
            }
            Ok(())
        }
        HollywoodKind::PermitRevoked { id, .. } => {
            if role != "user" {
                return Err(role_bound(role, "the user"));
            }
            if state.permits.contains_key(id) {
                Ok(())
            } else {
                Err(HRefusal::BadCard(format!("no active permit '{id}'")))
            }
        }
        HollywoodKind::PermitUsed {
            grants,
            role: used_role,
        } => {
            if role != "stage" {
                return Err(role_bound(role, "the stage"));
            }
            if used_role != HOLLYWOOD {
                return Err(HRefusal::BadCard(
                    "a Hollywood permit use names another role".into(),
                ));
            }
            if grants.iter().all(|id| state.permits.contains_key(id)) {
                Ok(())
            } else {
                Err(HRefusal::BadCard(
                    "a permit use names a missing grant".into(),
                ))
            }
        }
        // A prompt on Hollywood's own screen is the user's (D9): only
        // the user records the answer the stage then sends.
        HollywoodKind::RoleApproval { .. } => {
            if role != "user" {
                return Err(role_bound(role, "the user"));
            }
            Ok(())
        }
        HollywoodKind::MessageDelivered { target }
        | HollywoodKind::MessageDropped { target, .. } => {
            if role != "stage" {
                return Err(role_bound(role, "the stage"));
            }
            match state.messages.iter().find(|m| m.seq == *target) {
                None => Err(HRefusal::BadMessage(format!("no message '{target}'"))),
                Some(m) if m.delivered => Err(HRefusal::BadMessage(format!(
                    "message '{target}' is delivered"
                ))),
                Some(m) if m.dropped => Err(HRefusal::BadMessage(format!(
                    "message '{target}' is dropped"
                ))),
                Some(m) if m.seen => {
                    Err(HRefusal::BadMessage(format!("message '{target}' is seen")))
                }
                Some(_) => Ok(()),
            }
        }
        HollywoodKind::MessageSeen { target, .. } => {
            if role != HOLLYWOOD && role != "stage" {
                return Err(role_bound(role, "hollywood or the stage"));
            }
            match state.messages.iter().find(|m| m.seq == *target) {
                None => Err(HRefusal::BadMessage(format!("no message '{target}'"))),
                Some(m) if m.from != "user" || m.to != HOLLYWOOD => Err(HRefusal::BadMessage(
                    format!("message '{target}' is not a user tell to hollywood"),
                )),
                Some(m) if m.delivered => Err(HRefusal::BadMessage(format!(
                    "message '{target}' is delivered"
                ))),
                Some(m) if m.dropped => Err(HRefusal::BadMessage(format!(
                    "message '{target}' is dropped"
                ))),
                Some(m) if m.seen => {
                    Err(HRefusal::BadMessage(format!("message '{target}' is seen")))
                }
                Some(_) => Ok(()),
            }
        }
        // The user restarts the overseer from the TUI's menu: stop
        // `bl-hollywood` and launch again. Refused with no
        // `[hollywood]` to start, or while one is already pending.
        HollywoodKind::RoleRestart { .. } => {
            if role != "user" {
                return Err(role_bound(role, "the user"));
            }
            if state.role.restart.is_some() {
                return Err(HRefusal::BadMessage(
                    "a restart is already pending for hollywood".into(),
                ));
            }
            Ok(())
        }
        HollywoodKind::RoleProgress { text } => {
            if role != HOLLYWOOD {
                return Err(role_bound(role, "hollywood"));
            }
            crate::progress::valid_progress_text(text)
                .map_err(|msg| HRefusal::BadReport(msg.into()))
        }
    }
}
