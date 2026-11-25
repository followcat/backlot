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
        (CardAction::Setup { .. }, CardBase::HollywoodIdentity { .. }) => Ok(()),
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
        CardBase::HollywoodIdentity { .. } => "hollywood_identity",
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
        CardAction::Setup { .. } => "setup",
    }
}

/// The append-time rules for Hollywood's ledger. The role rules of
/// D17a/R26 run here so every append path is covered; replay stays
/// permissive.
pub fn validate(state: &HollywoodState, new: &NewHollywoodEvent) -> Result<(), HRefusal> {
    let role = new.by.role.as_str();
    match &new.kind {
        HollywoodKind::CardProposed { action, why, base } => {
            if role != HOLLYWOOD {
                return Err(role_bound(role, "hollywood"));
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
