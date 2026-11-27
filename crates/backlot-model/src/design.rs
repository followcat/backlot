//! Opening design discussion types and state projection (`design.*` events).
//!
//! Pure data and deterministic reductions over `design.opened`,
//! `design.entry`, and `design.signed`.

use std::fmt;

use serde::{Deserialize, Serialize};

use crate::state::StudioState;

/// Studio-level opening design gate policy (`cast.toml` / `studio.created`).
///
/// Missing fields in old logs deserialize as [`DesignGate::Off`]. Newly
/// created studios default to [`DesignGate::Required`] via
/// [`DesignGate::default_for_new_studio`].
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DesignGate {
    Required,
    #[default]
    Off,
}

impl DesignGate {
    /// Returns `true` when `self` is [`DesignGate::Off`].
    pub fn is_off(&self) -> bool {
        *self == Self::Off
    }

    /// Default opening design gate for newly created studios and `cast.toml`
    /// definitions that omit `design_gate`.
    pub const fn default_for_new_studio() -> Self {
        Self::Required
    }
}

impl fmt::Display for DesignGate {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            DesignGate::Required => f.write_str("required"),
            DesignGate::Off => f.write_str("off"),
        }
    }
}

/// Which slot a `design.entry` event writes into.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DesignSlot {
    Proposal,
    Buildability,
    Summary,
    UserInput,
    Supplement,
}

/// How a `UserInput` entry reached the opening design record.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EntrySource {
    Tell,
    Hook,
    ProducerNote,
}

/// How the person closed the opening design gate for a requirement version.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SignMode {
    Approved,
    Skipped,
}

/// One structured decision question attached to a round's `Summary` entry.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DesignQuestion {
    pub id: String,
    pub prompt: String,
    pub recommendation: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub section_ref: Option<String>,
}

/// Lifecycle phase of an opening design discussion for one requirement version.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DesignPhase {
    #[default]
    Idle,
    SuggestSkip,
    WaitingProposal,
    WaitingBuildability,
    WaitingSummary,
    WaitingUser,
    Signed,
}

impl DesignPhase {
    /// Wire/display name in `snake_case`.
    pub fn as_str(self) -> &'static str {
        match self {
            DesignPhase::Idle => "idle",
            DesignPhase::SuggestSkip => "suggest_skip",
            DesignPhase::WaitingProposal => "waiting_proposal",
            DesignPhase::WaitingBuildability => "waiting_buildability",
            DesignPhase::WaitingSummary => "waiting_summary",
            DesignPhase::WaitingUser => "waiting_user",
            DesignPhase::Signed => "signed",
        }
    }

    /// True when the user must respond (`waiting_user` or `suggest_skip`).
    pub fn is_waiting_user(self) -> bool {
        matches!(self, DesignPhase::SuggestSkip | DesignPhase::WaitingUser)
    }
}

impl fmt::Display for DesignPhase {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// One folded `design.entry` record stored in a [`DesignRound`].
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DesignEntryRecord {
    pub seq: u64,
    pub ts_ms: u64,
    pub by: String,
    pub slot: DesignSlot,
    pub file: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source: Option<EntrySource>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub refs: Vec<u64>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub questions: Vec<DesignQuestion>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub checks: Vec<crate::event::CheckDecl>,
}

/// One discussion round for a requirement version.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct DesignRound {
    pub round: u32,
    pub opened_seq: u64,
    pub opened_ts_ms: u64,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub entries: Vec<DesignEntryRecord>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub summary_seq: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub waiting_since_ms: Option<u64>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub questions: Vec<DesignQuestion>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub checks: Vec<crate::event::CheckDecl>,
}

/// Reduced opening design state for the active requirement version.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct DesignState {
    pub version: u32,
    pub round: u32,
    pub phase: DesignPhase,
    #[serde(default, skip_serializing_if = "crate::event::is_false")]
    pub suggest_skip: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub skip_reason: Option<String>,
    pub doc_file: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub rounds: Vec<DesignRound>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub waiting_since_ms: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sign_mode: Option<SignMode>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sign_note: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub signed_seq: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub signed_ts_ms: Option<u64>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub proposed_checks: Vec<crate::event::CheckDecl>,
}

impl DesignState {
    /// Current design phase.
    pub fn phase(&self) -> DesignPhase {
        self.phase
    }

    /// Current round number.
    pub fn current_round_num(&self) -> u32 {
        self.round
    }

    /// The active round record, if one has opened.
    pub fn current_round(&self) -> Option<&DesignRound> {
        self.rounds
            .iter()
            .rev()
            .find(|r| r.round == self.round)
            .or_else(|| self.rounds.last())
    }

    /// Open questions awaiting the user in `WaitingUser`.
    pub fn open_questions(&self) -> &[DesignQuestion] {
        if self.phase == DesignPhase::WaitingUser {
            self.current_round()
                .map(|r| r.questions.as_slice())
                .unwrap_or(&[])
        } else {
            &[]
        }
    }

    /// Seq of the latest `Summary` entry in the current round, if any.
    pub fn latest_summary_seq(&self) -> Option<u64> {
        self.current_round()
            .and_then(|r| r.summary_seq)
            .or_else(|| self.rounds.iter().rev().find_map(|r| r.summary_seq))
    }

    /// Whether a `UserInput` entry landed after the latest `Summary` in the current round.
    pub fn has_user_input_after_last_summary(&self) -> bool {
        let Some(round) = self.current_round() else {
            return false;
        };
        let Some(sum_seq) = round.summary_seq else {
            return false;
        };
        round
            .entries
            .iter()
            .any(|e| e.slot == DesignSlot::UserInput && e.seq > sum_seq)
    }

    /// `(seq, file)` for the current summary entry, when the stage needs its preview line.
    pub fn active_summary_file(&self) -> Option<(u64, &str)> {
        let sum_seq = self.latest_summary_seq()?;
        self.rounds.iter().rev().find_map(|r| {
            r.entries
                .iter()
                .rev()
                .find(|e| e.seq == sum_seq && e.slot == DesignSlot::Summary)
                .map(|e| (e.seq, e.file.as_str()))
        })
    }
}

/// Return `(seq, file)` for the active design's latest summary, if any.
pub fn design_summary_file(state: &StudioState) -> Option<(u64, String)> {
    let d = state.design.as_ref()?;
    let (seq, file) = d.active_summary_file()?;
    Some((seq, file.to_string()))
}
