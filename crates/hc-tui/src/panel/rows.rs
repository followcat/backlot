//! The panel's row model: a pure `(Summary, panel state)` → rows map.
//! The summary's own order is kept — it already sorts people by
//! priority and puts closed people last; the panel only groups and
//! collapses on top of it.

use backlot_model::summary::{RoleSummary, StudioSummary, Summary, Work};

use super::{Panel, PanelMode};
use crate::chrome::palette::Palette;
use crate::chrome::status::{AgentStatus, status_color};
use ratatui::style::Color;

/// Role groups in display order. `producer`/`director`/`writer` are
/// the fixed role keys; every other role is an actor.
pub(crate) const GROUPS: [&str; 4] = ["producer", "director", "writer", "actors"];

pub(crate) fn group_of(role: &RoleSummary) -> &'static str {
    match role.role.as_str() {
        "producer" => "producer",
        "director" => "director",
        "writer" => "writer",
        _ => "actors",
    }
}

/// A selected/clicked row's stable identity. Rows rebuild per frame,
/// so anything the user can act on is named by document ids, never by
/// position.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Target {
    Title,
    /// The "stage not running" line — shown, not actionable.
    StageDown,
    /// The empty-state line — shown, not actionable.
    Empty,
    Idle,
    Studio(String),
    /// `(studio, group)` — an expanded-mode group header.
    Group(String, String),
    /// A collapsed-mode "working" group row (aggregated over studios).
    WorkingGroup(String),
    /// `(studio, role)` — a person.
    Person(String, String),
}

/// One display row, borrowing its data from the summary.
#[derive(Debug)]
pub(crate) enum Row<'a> {
    Title,
    StageDown,
    /// No `backlot` document or an unparseable one — "no studios".
    Empty,
    /// Nothing working (collapsed mode's dim row).
    Idle,
    Studio(&'a StudioSummary),
    Group {
        studio: &'a StudioSummary,
        group: &'static str,
        collapsed: bool,
        count: usize,
    },
    Person {
        studio: &'a StudioSummary,
        role: &'a RoleSummary,
        /// The two-step reopen hint is armed on this row.
        confirm: bool,
    },
    /// Collapsed mode: one line per group with someone working.
    WorkingGroup {
        group: &'static str,
        working: usize,
    },
}

impl Row<'_> {
    /// The row's stable identity for selection, clicks and Enter.
    pub fn target(&self) -> Target {
        match self {
            Row::Title => Target::Title,
            Row::StageDown => Target::StageDown,
            Row::Empty => Target::Empty,
            Row::Idle => Target::Idle,
            Row::Studio(studio) => Target::Studio(studio.id.clone()),
            Row::Group { studio, group, .. } => Target::Group(studio.id.clone(), (*group).into()),
            Row::WorkingGroup { group, .. } => Target::WorkingGroup((*group).into()),
            Row::Person { studio, role, .. } => {
                Target::Person(studio.id.clone(), role.role.clone())
            }
        }
    }

    /// Rendered lines: a person may carry a `doing` line, an attention
    /// line, a `last turn failed` line and the armed reopen hint; a
    /// studio may carry an `error` line and a `next` step line; every
    /// other row is one line.
    pub fn height(&self) -> u16 {
        match self {
            Row::Studio(studio) => {
                1 + u16::from(studio.error.is_some()) + u16::from(studio.next.is_some())
            }
            Row::Person { role, confirm, .. } => {
                1 + u16::from(role.doing.is_some())
                    + u16::from(role.attention.is_some())
                    + u16::from(role.failed.is_some())
                    + u16::from(*confirm)
            }
            _ => 1,
        }
    }
}

/// The five work states the panel distinguishes (R17). Each has its
/// own mark *and* its own `AgentStatus` color — never a hardcoded
/// color.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Mark {
    Working,
    Waiting,
    Blocked,
    Lost,
    Closed,
}

impl Mark {
    /// A person's state word + work sample → the shown mark. A live
    /// person with no (or a degraded) sample reads as waiting; lost and
    /// closed override any sample.
    pub fn of(role: &RoleSummary) -> Mark {
        match role.state.as_str() {
            "closed" => Mark::Closed,
            "lost" => Mark::Lost,
            _ => match role.work {
                Some(Work::Working) => Mark::Working,
                Some(Work::Blocked) => Mark::Blocked,
                _ => Mark::Waiting,
            },
        }
    }
}

/// The mark's color, mapped through `chrome/status.rs` like every
/// other status indicator: working yellow, waiting green, blocked
/// red, lost dim, closed teal.
pub(crate) fn mark_color(mark: Mark, palette: &Palette) -> Color {
    status_color(
        match mark {
            Mark::Working => AgentStatus::Working,
            Mark::Waiting => AgentStatus::Idle,
            Mark::Blocked => AgentStatus::Blocked,
            Mark::Lost => AgentStatus::Unknown,
            Mark::Closed => AgentStatus::Done,
        },
        palette,
    )
}

/// Spinner frames for a working mark (~8.3 fps at the 120 ms tick).
const SPINNER: [&str; 10] = ["⠋", "⠙", "⠹", "⠸", "⠼", "⠴", "⠦", "⠧", "⠇", "⠏"];

/// The mark's symbol for spinner frame `spin`.
pub(crate) fn mark_symbol(mark: Mark, spin: usize) -> &'static str {
    match mark {
        Mark::Working => SPINNER[spin % SPINNER.len()],
        Mark::Waiting => "○",
        Mark::Blocked => "×",
        Mark::Lost => "!",
        Mark::Closed => "✓",
    }
}

/// The title line (R17): names what is working — each group with a
/// working person, `actors 2` style — or `idle` when nothing is.
pub(crate) fn title_text(summary: Option<&Summary>) -> String {
    let working = |group: &str| -> usize {
        summary
            .map(|s| {
                s.studios
                    .iter()
                    .flat_map(|st| &st.roles)
                    .filter(|r| group_of(r) == group && r.work == Some(Work::Working))
                    .count()
            })
            .unwrap_or(0)
    };
    let parts: Vec<String> = GROUPS
        .iter()
        .filter_map(|group| match working(group) {
            0 => None,
            1 => Some((*group).into()),
            n => Some(format!("{group} {n}")),
        })
        .collect();
    match parts.is_empty() {
        true => "idle".into(),
        false => format!("working: {}", parts.join(", ")),
    }
}

/// The whole row list for the current mode (R17/R18). `None` summary
/// — a missing, malformed or foreign-schema document — is the
/// "no studios" state, never an error row.
pub(crate) fn rows<'a>(
    summary: Option<&'a Summary>,
    stage_running: bool,
    panel: &Panel,
) -> Vec<Row<'a>> {
    let mut out = vec![Row::Title];
    if !stage_running {
        out.push(Row::StageDown);
    }
    let Some(summary) = summary else {
        out.push(Row::Empty);
        return out;
    };
    match panel.mode {
        PanelMode::Closed | PanelMode::Collapsed => {
            // One row per group with someone working; `idle` when none.
            let mut any = false;
            for group in GROUPS {
                let working = summary
                    .studios
                    .iter()
                    .flat_map(|s| &s.roles)
                    .filter(|r| group_of(r) == group && r.work == Some(Work::Working))
                    .count();
                if working > 0 {
                    any = true;
                    out.push(Row::WorkingGroup { group, working });
                }
            }
            if !any {
                out.push(Row::Idle);
            }
        }
        PanelMode::Expanded => {
            for studio in &summary.studios {
                out.push(Row::Studio(studio));
                for group in GROUPS {
                    let people: Vec<&RoleSummary> = studio
                        .roles
                        .iter()
                        .filter(|r| group_of(r) == group)
                        .collect();
                    if people.is_empty() {
                        continue;
                    }
                    let collapsed = panel
                        .collapsed_groups
                        .contains(&(studio.id.clone(), group.into()));
                    out.push(Row::Group {
                        studio,
                        group,
                        collapsed,
                        count: people.len(),
                    });
                    if collapsed {
                        continue;
                    }
                    for role in people {
                        out.push(Row::Person {
                            studio,
                            role,
                            confirm: panel
                                .reopen_confirm
                                .as_ref()
                                .is_some_and(|(s, r)| s == &studio.id && r == &role.role),
                        });
                    }
                }
            }
        }
    }
    out
}
