//! Core-document request/result and catalog wire types (B1a).
//!
//! Additive-only (`apiVersion 1`): structs never reject unknown fields, and
//! individual queue entries parse lazily so an unknown action tag yields a
//! per-request error ack without wedging sibling requests.

use serde::{Deserialize, Deserializer, Serialize};
use serde_json::Value;

use crate::event::{Priority, StudioMode};
pub use crate::req_catalog::*;
pub use crate::req_results::*;

pub const REQ_DOC_NAME: &str = "plugin.backlot.req";
pub const RES_DOC_NAME: &str = "plugin.backlot.res";

pub const REQ_SCHEMA: &str = "backlot-req/1";
pub const RES_SCHEMA: &str = "backlot-res/1";

pub const MAX_REQ_QUEUE: usize = 32;
pub const MAX_RES_ACKS: usize = 64;
pub const MAX_RESULT_BYTES: usize = 65_536;
pub const MAX_DOC_BYTES: usize = 1_048_576;

pub const REQ_TTL_MS: u64 = 60_000;
pub const REQ_FUTURE_SKEW_MS: u64 = 60_000;
pub const DEDUPE_TTL_MS: u64 = 24 * 60 * 60 * 1000;

/// Match herdr-core's document name rule: `^[a-z0-9_][a-z0-9_.-]{0,31}$`.
pub fn is_valid_doc_name(name: &str) -> bool {
    let bytes = name.as_bytes();
    if bytes.is_empty() || bytes.len() > 32 {
        return false;
    }
    let first = bytes[0];
    if !matches!(first, b'a'..=b'z' | b'0'..=b'9' | b'_') {
        return false;
    }
    bytes[1..]
        .iter()
        .all(|b| matches!(b, b'a'..=b'z' | b'0'..=b'9' | b'_' | b'.' | b'-'))
}

/// Validate a request timestamp against the stage clock (`60s` expiry window,
/// `60s` future skew allowance).
pub fn validate_timestamp(req_ts_ms: u64, now_ms: u64) -> Result<(), &'static str> {
    if req_ts_ms < now_ms && now_ms - req_ts_ms > REQ_TTL_MS {
        return Err("request expired");
    }
    if req_ts_ms > now_ms && req_ts_ms - now_ms > REQ_FUTURE_SKEW_MS {
        return Err("request timestamp too far in the future");
    }
    Ok(())
}

/// Document written by clients to request person actions (`plugin.backlot.req`).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ReqDoc {
    pub schema: String,
    #[serde(default)]
    pub queue: Vec<Value>,
}

impl Default for ReqDoc {
    fn default() -> Self {
        ReqDoc {
            schema: REQ_SCHEMA.into(),
            queue: Vec::new(),
        }
    }
}

/// Single queued request item in `plugin.backlot.req`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ReqEnvelope {
    pub req_id: String,
    pub ts_ms: u64,
    pub action: Value,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub confirm_token: Option<String>,
}

impl ReqEnvelope {
    pub fn new(
        req_id: impl Into<String>,
        ts_ms: u64,
        action: &ReqAction,
        confirm_token: Option<String>,
    ) -> Result<Self, serde_json::Error> {
        Ok(ReqEnvelope {
            req_id: req_id.into(),
            ts_ms,
            action: serde_json::to_value(action)?,
            confirm_token,
        })
    }

    pub fn parse_action(&self) -> Result<ReqAction, String> {
        parse_action_value(&self.action)
    }
}

/// Parse an individual `action` JSON value into [`ReqAction`], returning clear
/// refusal errors for explicit forbidden actions (`approve`, dialogs, outward
/// commands) and unknown tags instead of panicking or failing sibling entries.
pub fn parse_action_value(value: &Value) -> Result<ReqAction, String> {
    let tag = value
        .get("kind")
        .and_then(Value::as_str)
        .ok_or_else(|| "missing action kind".to_string())?;
    match tag {
        "permission" | "dialog" | "answer" | "prompt" | "prompt-answer" | "role-approval" => {
            return Err(
                "prompts and dialogs must be answered in the agent terminal, not via request doc"
                    .into(),
            );
        }
        "exec" | "shell" | "run" | "terminal" | "terminal-send" | "outward" => {
            return Err(format!(
                "refused action '{tag}': outward/shell actions are not allowed"
            ));
        }
        _ => {}
    }
    let action = serde_json::from_value::<ReqAction>(value.clone())
        .map_err(|e| format!("unsupported or malformed action '{tag}': {e}"))?;
    if let ReqAction::Approve {
        studio,
        person,
        scope,
        ..
    } = &action
    {
        if !matches!(scope.as_str(), "once" | "role" | "studio") {
            return Err(format!("unsupported approval scope '{scope}'"));
        }
        if person == "hollywood" && studio.is_some() {
            return Err("Hollywood approval carries no studio".into());
        }
        if person != "hollywood" && studio.is_none() {
            return Err("studio approval needs a studio".into());
        }
    }
    Ok(action)
}

fn de_card_id<'de, D>(deserializer: D) -> Result<u64, D::Error>
where
    D: Deserializer<'de>,
{
    #[derive(Deserialize)]
    #[serde(untagged)]
    enum CardId {
        Num(u64),
        Str(String),
    }
    match CardId::deserialize(deserializer)? {
        CardId::Num(n) => Ok(n),
        CardId::Str(s) => s
            .strip_prefix('c')
            .unwrap_or(&s)
            .parse::<u64>()
            .map_err(|_| serde::de::Error::custom(format!("bad card id '{s}'"))),
    }
}

fn approval_scope_once() -> String {
    "once".into()
}

/// Typed B1a person actions accepted over `plugin.backlot.req`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "kebab-case")]
pub enum ReqAction {
    Tell {
        studio: String,
        role: String,
        text: String,
    },
    TellHollywood {
        text: String,
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        answers: Vec<u64>,
    },
    CardApply {
        #[serde(deserialize_with = "de_card_id")]
        id: u64,
    },
    CardCancel {
        #[serde(deserialize_with = "de_card_id")]
        id: u64,
        #[serde(default, skip_serializing_if = "String::is_empty")]
        reason: String,
    },
    CardShow {
        #[serde(deserialize_with = "de_card_id")]
        id: u64,
    },
    AttentionClear {
        studio: String,
        #[serde(alias = "seq")]
        target: u64,
    },
    /// `approve` through the server: `scope` is once, role or studio.
    Approve {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        studio: Option<String>,
        person: String,
        decision: crate::event::ApprovalDecision,
        #[serde(default = "approval_scope_once")]
        scope: String,
    },
    /// `permit list` through the server.
    PermitList {
        studio: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        role: Option<String>,
    },
    /// `permit log` through the server.
    PermitLog { studio: String, role: String },
    /// `permit revoke` through the server.
    PermitRevoke { studio: String, id: String },
    AskAcknowledge {
        studio: String,
        role: String,
        target: u64,
    },
    Recast {
        studio: String,
        #[serde(alias = "role")]
        person: String,
        runner: String,
        model: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        effort: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        permission_profile: Option<String>,
        #[serde(default, skip_serializing_if = "String::is_empty")]
        reason: String,
    },
    Restart {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        studio: Option<String>,
        #[serde(alias = "role")]
        person: String,
        #[serde(default, skip_serializing_if = "crate::event::is_false")]
        fresh: bool,
        #[serde(default, skip_serializing_if = "String::is_empty")]
        reason: String,
    },
    Close {
        studio: String,
        #[serde(alias = "role")]
        person: String,
    },
    Reopen {
        studio: String,
        #[serde(alias = "role")]
        person: String,
    },
    TaskPriority {
        studio: String,
        task: String,
        level: Priority,
    },
    TaskPause {
        studio: String,
        task: String,
        #[serde(default, skip_serializing_if = "String::is_empty")]
        reason: String,
    },
    TaskResume {
        studio: String,
        task: String,
        #[serde(default, skip_serializing_if = "String::is_empty")]
        reason: String,
    },
    TaskAbandon {
        studio: String,
        task: String,
        #[serde(default, skip_serializing_if = "String::is_empty")]
        reason: String,
    },
    HollywoodSetup {
        runner: String,
        model: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        effort: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        key: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        default_cast: Option<String>,
        #[serde(default, skip_serializing_if = "crate::event::is_false")]
        force_offline: bool,
    },
    /// Switch Hollywood's agent by make-before-break handoff.
    HollywoodSwitch {
        runner: String,
        model: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        effort: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        timeout_s: Option<u64>,
        #[serde(default, skip_serializing_if = "std::ops::Not::not")]
        force: bool,
    },
    /// Cancel an active Hollywood switch handoff.
    HollywoodSwitchCancel,
    /// Query active or recent Hollywood switch handoff status.
    HollywoodSwitchStatus,
    /// `studio new`: the cast and the first message travel as content,
    /// never as client paths — the hub validates and canonicalizes
    /// `repo` against its own filesystem.
    StudioNew {
        id: String,
        title: String,
        repo: String,
        cast: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        mode: Option<StudioMode>,
        #[serde(default, skip_serializing_if = "crate::event::is_false")]
        init: bool,
        #[serde(default, skip_serializing_if = "crate::event::is_false")]
        no_git: bool,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        ask: Option<String>,
        #[serde(default, skip_serializing_if = "crate::event::is_false")]
        dry_run: bool,
    },
    StudioFinish {
        studio: String,
        #[serde(default, skip_serializing_if = "String::is_empty")]
        reason: String,
        #[serde(default, skip_serializing_if = "crate::event::is_false")]
        force: bool,
    },
    /// `studio reopen`: the person brings a finished studio back.
    StudioReopen {
        studio: String,
        #[serde(default, skip_serializing_if = "String::is_empty")]
        reason: String,
    },
    /// `studio cast show`: the effective cast as a cast file (or the
    /// settings as JSON). Read-only; answered from the hub.
    StudioCastShow {
        studio: String,
        #[serde(default, skip_serializing_if = "crate::event::is_false")]
        json: bool,
    },
    /// `studio cast set`: the cast file travels as **content**, never as a
    /// client path — the hub writes it to a temp file and parses it with
    /// `studio new`'s own rules. `dry_run` returns the diff and records
    /// nothing.
    StudioCastSet {
        studio: String,
        cast: String,
        #[serde(default, skip_serializing_if = "crate::event::is_false")]
        dry_run: bool,
        #[serde(default, skip_serializing_if = "crate::event::is_false")]
        json: bool,
    },
    StudioPrune {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        studio: Option<String>,
        #[serde(default, skip_serializing_if = "crate::event::is_false")]
        all_finished: bool,
        #[serde(default, skip_serializing_if = "crate::event::is_false")]
        dry_run: bool,
        #[serde(default, skip_serializing_if = "crate::event::is_false")]
        force: bool,
    },
    StudioDelete {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        studio: Option<String>,
        #[serde(default, skip_serializing_if = "crate::event::is_false")]
        dry_run: bool,
        #[serde(default, skip_serializing_if = "crate::event::is_false")]
        force: bool,
    },
    StudioRestore {
        archived: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        name: Option<String>,
        #[serde(default, skip_serializing_if = "crate::event::is_false")]
        dry_run: bool,
    },
    /// The New Studio dialog's directory probe, answered from the hub.
    Repos {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        from: Option<String>,
        #[serde(default, skip_serializing_if = "crate::event::is_false")]
        all: bool,
    },
    /// The New Studio dialog's runner probe, answered from the hub.
    Runners,
    /// The dialog's `--version` probe, answered from the hub.
    Version,
    /// `design sign`: user-only opening design sign-off (`approved` or `skipped`).
    DesignSign {
        studio: String,
        #[serde(default, skip_serializing_if = "crate::event::is_false")]
        skip: bool,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        reason: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        note: Option<String>,
        #[serde(default, skip_serializing_if = "crate::event::is_false")]
        no_checks: bool,
    },
    /// `design start round`: reject `--suggest-skip` and start Round 1 immediately.
    DesignStartRound { studio: String },
    /// Install or refresh the machine-local Agent Skill for the named
    /// agents (or every detected agent when the list is empty). `force`
    /// is the CLI's `--force`: it replaces a foreign copy.
    #[serde(rename = "skill.install")]
    SkillInstall {
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        agents: Vec<String>,
        #[serde(default, skip_serializing_if = "crate::event::is_false")]
        force: bool,
        /// The declared caller when a CLI bridge submits this for a role.
        /// The TUI leaves it empty, which is the person's path; a named
        /// role is refused by the stage.
        #[serde(default, rename = "as", skip_serializing_if = "Option::is_none")]
        as_role: Option<String>,
    },
    /// Uninstall the machine-local Agent Skill for the named agents
    /// (or every known agent when the list is empty). Foreign copies
    /// are always left alone.
    #[serde(rename = "skill.uninstall")]
    SkillUninstall {
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        agents: Vec<String>,
        #[serde(default, rename = "as", skip_serializing_if = "Option::is_none")]
        as_role: Option<String>,
    },
}

/// Ack document written solely by the stage (`plugin.backlot.res`).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ResDoc {
    pub schema: String,
    #[serde(default)]
    pub acks: Vec<ResAck>,
}

impl Default for ResDoc {
    fn default() -> Self {
        ResDoc {
            schema: RES_SCHEMA.into(),
            acks: Vec::new(),
        }
    }
}

/// Single ack item in `plugin.backlot.res`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ResAck {
    pub req_id: String,
    pub ts_ms: u64,
    pub ok: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub result: Option<Value>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

impl ResAck {
    /// Build a success ack, enforcing [`MAX_RESULT_BYTES`] on the serialized JSON
    /// payload and degrading cleanly to an error ack if exceeded.
    pub fn ok_value(req_id: impl Into<String>, ts_ms: u64, result: Value) -> Self {
        let req_id = req_id.into();
        let bytes = serde_json::to_vec(&result)
            .map(|v| v.len())
            .unwrap_or(usize::MAX);
        if bytes > MAX_RESULT_BYTES {
            return ResAck::err(
                req_id,
                ts_ms,
                format!("result exceeds {MAX_RESULT_BYTES} byte limit ({bytes} bytes)"),
            );
        }
        ResAck {
            req_id,
            ts_ms,
            ok: true,
            result: Some(result),
            error: None,
        }
    }

    pub fn err(req_id: impl Into<String>, ts_ms: u64, error: impl Into<String>) -> Self {
        ResAck {
            req_id: req_id.into(),
            ts_ms,
            ok: false,
            result: None,
            error: Some(error.into()),
        }
    }
}
