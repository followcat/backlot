//! The Call Sheet body: every item across every machine that waits
//! on the user, sorted by urgency — user-owned prompts first, then
//! stopped stages and unreachable machines with live studios, then
//! open cards, then studios ready to finish. Pure functions of the
//! `backlot` documents plus the watcher's machine list and
//! inventory: nothing is scraped, and a role's `attention_user`
//! flag (D9) is what puts its prompt here — the director's own
//! prompts never enter the sheet.

use backlot_model::summary::{CardSummary, RoleSummary, StudioSummary};
use hc_proto::watch::View;

use crate::panel::data::{MachineSummary, stage_running};
use crate::studio::StudioKey;

/// A machine's line, as `machine.list` last reported it — `None`
/// (this core) is always online.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Conn {
    /// `connected` — or this core.
    Online,
    /// `connecting` — the core is still reaching it.
    Reconnecting,
    /// Anything else — `unreachable`, `needs_login`, a vanished row.
    Offline,
}

/// How the row model learns a machine's line and whether its
/// `backlot-stage` answers — the app binds `view`, tests bind
/// fixtures. `conn` then `stage_running`.
pub(crate) type Status<'a> = dyn Fn(Option<&str>) -> (Conn, bool) + 'a;

/// `view`'s read of a machine's line.
pub(crate) fn conn_of(view: &View, machine: Option<&str>) -> Conn {
    let Some(name) = machine else {
        return Conn::Online;
    };
    let state = view
        .machines
        .iter()
        .flatten()
        .find(|m| m.name == name)
        .map(|m| m.state.as_str())
        .unwrap_or("unreachable");
    match state {
        "connected" => Conn::Online,
        "connecting" => Conn::Reconnecting,
        _ => Conn::Offline,
    }
}

/// The status closure over `view` — every row asks it per machine.
pub(crate) fn status_of(view: &View) -> impl Fn(Option<&str>) -> (Conn, bool) + '_ {
    move |machine| (conn_of(view, machine), stage_running(view, machine))
}

/// A user-owned permission prompt (D9): `attention_user` only.
pub(crate) struct PromptItem<'a> {
    pub machine: Option<&'a str>,
    pub studio: &'a StudioSummary,
    pub role: &'a RoleSummary,
}

/// A machine whose stage is down — or that is unreachable itself —
/// while it has live studios. `studios` is its studio count.
pub(crate) struct StageItem<'a> {
    pub machine: Option<&'a str>,
    pub offline: bool,
    pub studios: usize,
}

/// A `ready_to_finish` studio.
pub(crate) struct ReadyItem<'a> {
    pub machine: Option<&'a str>,
    pub studio: &'a StudioSummary,
}

/// One waiting item in sort order.
pub(crate) enum Wait<'a> {
    Prompt(PromptItem<'a>),
    Stage(StageItem<'a>),
    Card(&'a CardSummary),
    Ready(ReadyItem<'a>),
}

impl Wait<'_> {
    /// The row's stable identity across publishes.
    pub fn target(&self) -> super::Target {
        match self {
            Wait::Prompt(item) => super::Target::Prompt(
                StudioKey {
                    machine: item.machine.map(str::to_string),
                    id: item.studio.id.clone(),
                },
                item.role.role.clone(),
            ),
            Wait::Stage(item) => {
                super::Target::Stage(item.machine.map(str::to_string), item.offline)
            }
            Wait::Card(card) => super::Target::Card(card.id.clone()),
            Wait::Ready(item) => super::Target::Ready(StudioKey {
                machine: item.machine.map(str::to_string),
                id: item.studio.id.clone(),
            }),
        }
    }

    /// Whether the row can offer a detail line — prompts and cards
    /// can; stage and ready rows are one line each.
    pub fn detailable(&self) -> bool {
        matches!(self, Wait::Prompt(_) | Wait::Card(_))
    }
}

/// Every waiting item across every machine, in the sheet's order:
/// prompts, stage issues, cards, ready studios — each group in
/// machine order, then document order.
pub(crate) fn waiting_items<'a>(
    machines: &'a [MachineSummary],
    status: &Status<'a>,
) -> Vec<Wait<'a>> {
    let mut out: Vec<Wait<'a>> = Vec::new();
    for m in machines {
        for studio in &m.summary.studios {
            for role in &studio.roles {
                if role.attention_user {
                    out.push(Wait::Prompt(PromptItem {
                        machine: m.machine.as_deref(),
                        studio,
                        role,
                    }));
                }
            }
        }
    }
    for m in machines {
        if m.summary.studios.is_empty() {
            continue;
        }
        let (conn, stage) = status(m.machine.as_deref());
        if conn == Conn::Offline {
            out.push(Wait::Stage(StageItem {
                machine: m.machine.as_deref(),
                offline: true,
                studios: m.summary.studios.len(),
            }));
        } else if !stage {
            out.push(Wait::Stage(StageItem {
                machine: m.machine.as_deref(),
                offline: false,
                studios: m.summary.studios.len(),
            }));
        }
    }
    // The cards stay this core's — they act on this core's stage
    // (the same read `band_rows` always made).
    if let Some(summary) = super::rows::local_summary(machines)
        && let Some(hollywood) = summary.hollywood.as_ref()
    {
        out.extend(hollywood.cards.iter().map(Wait::Card));
    }
    for m in machines {
        for studio in &m.summary.studios {
            if studio.ready_to_finish {
                out.push(Wait::Ready(ReadyItem {
                    machine: m.machine.as_deref(),
                    studio,
                }));
            }
        }
    }
    out
}

/// The machine a waiting row names: the remote name, or this core's
/// label — `local` only reads when studios span machines.
pub(crate) fn machine_label<'a>(machine: Option<&'a str>, local: &'a str, multi: bool) -> &'a str {
    match machine {
        Some(name) => name,
        None if multi => local,
        None => "",
    }
}

#[cfg(test)]
#[path = "tests/sheet_tests.rs"]
mod tests;
