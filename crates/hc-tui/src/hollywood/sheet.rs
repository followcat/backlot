//! The Call Sheet body: every item across every machine that waits
//! on the user, sorted by urgency — user-owned prompts first, then
//! stopped stages and unreachable machines with live studios, then
//! open cards, then studios ready to finish. Pure functions of the
//! `backlot` documents plus the watcher's machine list and
//! inventory: nothing is scraped, and a role's `attention_user`
//! flag (D9) is what puts its prompt here — the director's own
//! prompts never enter the sheet.

use std::collections::BTreeSet;

use backlot_model::summary::{CardSummary, OutwardCardSummary, RoleSummary, StudioSummary};
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

/// One open outward push/PR card projected onto a studio (push-perms §2.2).
pub(crate) struct OutwardItem<'a> {
    pub machine: Option<&'a str>,
    pub studio: &'a StudioSummary,
    pub card: &'a OutwardCardSummary,
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

/// A role that needs the user beyond a D9 prompt: a needs-you
/// attention or an unanswered question.
pub(crate) struct RoleItem<'a> {
    pub machine: Option<&'a str>,
    pub studio: &'a StudioSummary,
    pub role: &'a RoleSummary,
    /// `true` for the role's unanswered `tell user`; `false` for a
    /// needs-you attention.
    pub asks: bool,
}

/// A studio with no recorded requirement — the next step is telling
/// the producer what to build.
pub(crate) struct StudioItem<'a> {
    pub machine: Option<&'a str>,
    pub studio: &'a StudioSummary,
}

/// One waiting item in sort order.
pub(crate) enum Wait<'a> {
    Prompt(PromptItem<'a>),
    Role(RoleItem<'a>),
    /// Hollywood itself: a user-owned prompt (`attention_user`) or a
    /// blocked screen with no prompt.
    Hollywood(&'a backlot_model::summary::HollywoodSummary),
    Upgrade(StudioItem<'a>),
    NoRequirement(StudioItem<'a>),
    Stage(StageItem<'a>),
    Card(&'a CardSummary),
    Outward(OutwardItem<'a>),
    Ready(ReadyItem<'a>),
}

impl Wait<'_> {
    /// Whether this prompt row's recorded decision would actually reach
    /// the runner — `can_answer_prompt` on the role or Hollywood. A
    /// row that cannot answer offers only Enter and `p`.
    pub fn can_answer(&self) -> bool {
        match self {
            Wait::Prompt(item) => item.role.can_answer_prompt,
            Wait::Hollywood(hollywood) => {
                if let Some(hf) = &hollywood.handoff
                    && hf.phase == "needs_approval"
                {
                    return backlot_model::caps::get(&hf.target_runner)
                        .is_some_and(|c| c.can_answer());
                }
                hollywood.can_answer_prompt
            }
            _ => false,
        }
    }

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
            Wait::Role(item) => super::Target::Role(
                StudioKey {
                    machine: item.machine.map(str::to_string),
                    id: item.studio.id.clone(),
                },
                item.role.role.clone(),
            ),
            Wait::Hollywood(_) => super::Target::Hollywood,
            Wait::Upgrade(item) => super::Target::Upgrade(StudioKey {
                machine: item.machine.map(str::to_string),
                id: item.studio.id.clone(),
            }),
            Wait::NoRequirement(item) => super::Target::Tell(StudioKey {
                machine: item.machine.map(str::to_string),
                id: item.studio.id.clone(),
            }),
            Wait::Stage(item) => {
                super::Target::Stage(item.machine.map(str::to_string), item.offline)
            }
            Wait::Card(card) => super::Target::Card(card.id.clone()),
            Wait::Outward(item) => super::Target::OutwardCard(
                StudioKey {
                    machine: item.machine.map(str::to_string),
                    id: item.studio.id.clone(),
                },
                item.card.id.clone(),
            ),
            Wait::Ready(item) => super::Target::Ready(StudioKey {
                machine: item.machine.map(str::to_string),
                id: item.studio.id.clone(),
            }),
        }
    }

    /// Whether the row can offer a detail line — prompts, roles,
    /// Hollywood and cards always can; a ready row gets one while the
    /// wrap report is still owed (R3-3's dim `report pending`); stage
    /// and no-requirement rows are one line each.
    pub fn detailable(&self) -> bool {
        match self {
            Wait::Prompt(_)
            | Wait::Role(_)
            | Wait::Hollywood(_)
            | Wait::Card(_)
            | Wait::Outward(_)
            | Wait::NoRequirement(_) => true,
            Wait::Ready(item) => crate::panel::data::report_pending(item.studio),
            Wait::Stage(_) | Wait::Upgrade(_) => false,
        }
    }
}

/// Every waiting item across every machine, in the sheet's order:
/// user prompts, roles that need the user, Hollywood when it is
/// blocked, studios with no requirement, stage issues, cards, and
/// ready studios — each group in machine order, then document order.
/// This is the one "needs you" list the calm row, the lot pill and
/// the sheet read.
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
        for studio in &m.summary.studios {
            if studio.needs_upgrade {
                out.push(Wait::Upgrade(StudioItem {
                    machine: m.machine.as_deref(),
                    studio,
                }));
            }
        }
    }
    for m in machines {
        for studio in &m.summary.studios {
            for role in &studio.roles {
                if role.attention_user {
                    continue;
                }
                let asks = match crate::studio::glyphs::Person::of(role) {
                    crate::studio::glyphs::Person::NeedsYou => false,
                    crate::studio::glyphs::Person::Asks => true,
                    _ => continue,
                };
                out.push(Wait::Role(RoleItem {
                    machine: m.machine.as_deref(),
                    studio,
                    role,
                    asks,
                }));
            }
        }
    }
    // Hollywood's own state is this core's: an active or rolled-back
    // switch handoff, a user-owned prompt, quota/auth error, or blocked screen.
    if let Some(hw) = super::rows::local_summary(machines).and_then(|s| s.hollywood.as_ref())
        && (super::handoff_ui::is_handoff_waiting_visible(hw)
            || (hw.state == "live"
                && (hw.attention_user
                    || hw.work == Some(backlot_model::summary::Work::Blocked)
                    || super::handoff_ui::is_quota_or_error(hw))))
    {
        out.push(Wait::Hollywood(hw));
    }
    for m in machines {
        for studio in &m.summary.studios {
            if studio.requirement_version == 0
                && studio.state != "finished"
                && !studio.needs_upgrade
            {
                out.push(Wait::NoRequirement(StudioItem {
                    machine: m.machine.as_deref(),
                    studio,
                }));
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
    // Local outward cards on `StudioSummary` take the richer `Wait::Outward`
    // row (and carry machine ownership on remote hosts), so dedupe them
    // locally against `HollywoodSummary.cards`.
    let local_outward: BTreeSet<&str> = super::rows::local_summary(machines)
        .map(|s| {
            s.studios
                .iter()
                .flat_map(|st| st.outward_cards.iter().map(|c| c.id.as_str()))
                .collect()
        })
        .unwrap_or_default();
    if let Some(summary) = super::rows::local_summary(machines)
        && let Some(hollywood) = summary.hollywood.as_ref()
    {
        out.extend(
            hollywood
                .cards
                .iter()
                .filter(|c| !local_outward.contains(c.id.as_str()))
                .map(Wait::Card),
        );
    }
    for m in machines {
        for studio in &m.summary.studios {
            for card in &studio.outward_cards {
                out.push(Wait::Outward(OutwardItem {
                    machine: m.machine.as_deref(),
                    studio,
                    card,
                }));
            }
        }
    }
    for m in machines {
        for studio in &m.summary.studios {
            if studio.ready_to_finish && !studio.needs_upgrade {
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
