//! The panel's row model: a pure `(machines, Panel, current,
//! body height)` → one-line rows map. Every row is a single line — a
//! block border, a person, a task row, a cue or drawer line, or a
//! stacked block's `╶ id …` line — so the scroll math stays in
//! one-line steps.

use backlot_model::summary::{RoleSummary, TaskSummary};

use super::data::{MachineSummary, StudioRef};
use super::row_lines::{owed_text, person_drawer, task_drawer};
use super::rows_fit::*;
use super::{Panel, PanelMode};
use crate::studio::StudioKey;

/// A selected/clicked row's stable identity. Rows rebuild per frame,
/// so anything the user can act on is named by machine plus document
/// ids, never by position. Rows that only inform — the title,
/// dividers, block bottoms, cue and drawer lines — carry `None`
/// and are skipped by the cursor keys and clicks alike (A2.2).
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Target {
    /// Shown, not actionable.
    None,
    /// Any header row of an expanded block — the top border, the
    /// owed-step line. Enter/click makes the studio current.
    Studio(StudioKey),
    /// A stacked block's `╶ id …` line — Enter/click expands it.
    Block(StudioKey),
    /// A collapsed-strip row — Enter/click expands the panel.
    Strip(StudioKey),
    /// `(studio, role)` — a person row.
    Person(StudioKey, String),
    /// `(studio, task)` — a task row with no live actor row.
    Task(StudioKey, String),
    /// The `✓ <n> accepted` fold line — Space/Enter opens it.
    Fold(StudioKey),
}

impl Target {
    /// The studio a row target belongs to — the selected block's
    /// accent border and the header's `▸` mark follow it (A2.3/A7.2).
    pub(crate) fn studio(&self) -> Option<&StudioKey> {
        match self {
            Target::Studio(key)
            | Target::Block(key)
            | Target::Strip(key)
            | Target::Person(key, _)
            | Target::Task(key, _)
            | Target::Fold(key) => Some(key),
            Target::None => Option::None,
        }
    }
}

/// The owed-step subheader's kind — the glyph and colour follow it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Owed {
    /// `→ <label> · <owner name>` — a ledger step.
    Step,
    /// `→ finish studio · user [f]` — ready to finish.
    Finish,
    /// `! finish <id>? [y]` — `f` armed the inline confirm.
    Confirm,
    /// `◌ stage not running — [r] start`.
    StageDown,
    /// `× <machine> unreachable`.
    Offline,
    /// `! <error>` — the ledger/stage error.
    Error,
}

/// An extra line under a person.
#[derive(Debug)]
pub(crate) enum PersonLine {
    /// `! <attention>` or the prompt cue — the blocked colour.
    Cue,
    /// `! last turn failed: <e>` — only when no attention shows.
    Failed,
}

/// One display row, borrowing its data from the summaries.
#[derive(Debug)]
pub(crate) enum Row<'a> {
    Title,
    /// `stage not running` — one per machine whose document answered
    /// but whose stage is down; `machine` names it when several
    /// machines answered.
    StageDown {
        machine: Option<&'a str>,
    },
    /// `× <machine> unreachable` — a listed machine that is not
    /// connected; its studios cannot answer.
    Offline {
        machine: &'a str,
    },
    /// No `backlot` document anywhere or an unparseable one —
    /// "no studios".
    Empty,
    /// `╭─ <label> <health> ── r<ver> · <done>/<total> !<n> ─╮`.
    BlockTop {
        studio: StudioRef<'a>,
        health: crate::studio::glyphs::Health,
    },
    /// The owed-step subheader — one line under the header.
    Owed {
        studio: StudioRef<'a>,
        kind: Owed,
        text: String,
    },
    /// `╰──╯`
    BlockBottom(StudioRef<'a>),
    /// `│ <mark> <name> <task> <flag>   <state> │`
    Person {
        studio: StudioRef<'a>,
        role: &'a RoleSummary,
    },
    /// A person's attention-cue/failed/confirm/hint extra line.
    PersonLine {
        studio: StudioRef<'a>,
        role: &'a RoleSummary,
        kind: PersonLine,
    },
    /// A task with no live actor row: `│ · task t2 (waits t1)  dep │`.
    Task {
        studio: StudioRef<'a>,
        task: &'a TaskSummary,
    },
    /// `✓ <n> accepted` — the fold line when many tasks are done.
    AcceptedFold {
        studio: StudioRef<'a>,
        open: bool,
    },
    /// A `Space` drawer's content line under its owner row.
    Drawer {
        studio: StudioRef<'a>,
        text: String,
    },
    /// A stacked block's single line: `╶ <id> ⠿2 ○3 ×1 !<n>`.
    Stacked(StudioRef<'a>),
    /// The collapsed strip's studio line — `● id·machine  d/t`.
    Strip {
        studio: StudioRef<'a>,
        health: crate::studio::glyphs::Health,
    },
    /// A strip's active-or-blocked role line under its studio.
    StripRole {
        studio: StudioRef<'a>,
        role: &'a RoleSummary,
    },
    /// H4: `no hollywood — backlot hollywood setup …` — the expanded
    /// panel's last row while `[hollywood]` is absent (R22).
    SetupHint,
}

impl Row<'_> {
    /// The row's stable identity for selection, clicks and Enter.
    /// Owed/cue/drawer/border rows are `Target::None` (A2.2).
    pub fn target(&self) -> Target {
        match self {
            Row::Title
            | Row::StageDown { .. }
            | Row::Offline { .. }
            | Row::Empty
            | Row::Owed { .. }
            | Row::BlockBottom(_)
            | Row::Drawer { .. }
            | Row::SetupHint => Target::None,
            Row::BlockTop { studio, .. } => Target::Studio(studio.key()),
            Row::Person { studio, role } => Target::Person(studio.key(), role.role.clone()),
            Row::PersonLine { studio, role, .. } => Target::Person(studio.key(), role.role.clone()),
            Row::Task { studio, task } => Target::Task(studio.key(), task.id.clone()),
            Row::AcceptedFold { studio, .. } => Target::Fold(studio.key()),
            Row::Stacked(studio) => Target::Block(studio.key()),
            Row::Strip { studio, .. } | Row::StripRole { studio, .. } => {
                Target::Strip(studio.key())
            }
        }
    }

    /// Rendered lines — every row is one line, so scroll metrics stay
    /// in line steps.
    pub fn height(&self) -> u16 {
        1
    }
}

/// One studio's expanded block: header, owed-step subheader, live
/// roles, task rows, the accepted fold, closed roles, bottom border.
fn block_rows<'a>(
    out: &mut Vec<Row<'a>>,
    studio: StudioRef<'a>,
    panel: &Panel,
    stage: bool,
    reachable: bool,
) {
    let health = health_of(studio.studio, stage, reachable);
    out.push(Row::BlockTop { studio, health });
    let owed = if !reachable {
        Some((
            Owed::Offline,
            format!("{} unreachable", studio.machine.unwrap_or("local")),
        ))
    } else if !stage {
        Some((Owed::StageDown, "stage not running — [r] start".to_string()))
    } else if let Some(error) = &studio.studio.error {
        Some((Owed::Error, error.clone()))
    } else if let Some(next) = &studio.studio.next {
        Some((Owed::Step, owed_text(studio.studio, next)))
    } else if studio.studio.ready_to_finish {
        Some((Owed::Finish, "finish studio · user [f]".to_string()))
    } else {
        None
    };
    if let Some((kind, text)) = owed {
        out.push(Row::Owed { studio, kind, text });
    }
    // `f` armed the finish confirm — its `y`/`n` line sits under
    // the owed step, cleared by Esc or a selection move (S2 §2).
    if panel.finish_confirm.as_ref() == Some(&studio.key()) {
        out.push(Row::Owed {
            studio,
            kind: Owed::Confirm,
            text: format!("finish {}? [y/n]", studio.studio.id),
        });
    }
    let emit_person = |out: &mut Vec<Row<'a>>, role: &'a RoleSummary| {
        let key = studio.key();
        out.push(Row::Person { studio, role });
        if role.attention.is_some() {
            out.push(Row::PersonLine {
                studio,
                role,
                kind: PersonLine::Cue,
            });
        } else if role.failed.is_some() {
            out.push(Row::PersonLine {
                studio,
                role,
                kind: PersonLine::Failed,
            });
        }
        let target = Target::Person(key, role.role.clone());
        if panel.drawer.as_ref() == Some(&target) {
            for text in person_drawer(studio, role) {
                out.push(Row::Drawer { studio, text });
            }
        }
    };
    for role in live_roles(studio.studio) {
        emit_person(out, role);
    }
    let accepted_open = panel.accepted_open(&studio.key());
    let folds = fold_count(studio.studio);
    for task in task_rows(studio.studio, accepted_open) {
        out.push(Row::Task { studio, task });
        let target = Target::Task(studio.key(), task.id.clone());
        if panel.drawer.as_ref() == Some(&target) {
            for text in task_drawer(studio, task) {
                out.push(Row::Drawer { studio, text });
            }
        }
    }
    if folds >= 6 {
        out.push(Row::AcceptedFold {
            studio,
            open: accepted_open,
        });
    }
    for role in closed_roles(studio.studio) {
        emit_person(out, role);
    }
    out.push(Row::BlockBottom(studio));
}

/// The whole row list for the current mode. An empty `machines` — no
/// machine's document parsed — is the "no studios" state, never an
/// error row. `stage_running` answers per machine (`None` is this
/// core); `reachable` answers whether the machine itself is
/// connected.
/// The whole row list for the current mode. An empty `machines` — no
/// machine's document parsed — is the "no studios" state, never an
/// error row. `stage_running` answers per machine (`None` is this
/// core); `reachable` answers whether the machine itself is
/// connected.
pub(crate) fn rows<'a>(
    machines: &'a [MachineSummary],
    stage_running: impl Fn(Option<&str>) -> bool,
    reachable: impl Fn(Option<&str>) -> bool,
    panel: &Panel,
    current: Option<&StudioKey>,
    body_height: u16,
) -> Vec<Row<'a>> {
    let mut out = vec![Row::Title];
    if machines.is_empty() {
        if !stage_running(None) {
            out.push(Row::StageDown { machine: None });
        }
        out.push(Row::Empty);
        return out;
    }
    for machine in machines {
        // A machine's own line only when it has no studio blocks to
        // carry the state — inside a block the owed-step subheader
        // says the same thing (`◌ stage not running`, `× …unreachable`).
        if !machine.summary.studios.is_empty() {
            continue;
        }
        if let Some(name) = machine.machine.as_deref()
            && !reachable(Some(name))
        {
            out.push(Row::Offline { machine: name });
            continue;
        }
        if !stage_running(machine.machine.as_deref()) {
            out.push(Row::StageDown {
                machine: machine.machine.as_deref(),
            });
        }
    }
    let studios = super::data::studios(machines);
    match panel.mode {
        PanelMode::Closed | PanelMode::Collapsed => {
            for studio in &studios {
                let health = health_of(
                    studio.studio,
                    stage_running(studio.machine),
                    reachable(studio.machine),
                );
                out.push(Row::Strip {
                    studio: *studio,
                    health,
                });
                for role in strip_roles(studio.studio) {
                    out.push(Row::StripRole {
                        studio: *studio,
                        role,
                    });
                }
            }
        }
        PanelMode::Expanded => {
            let owed = |studio: StudioRef<'_>| {
                !reachable(studio.machine)
                    || !stage_running(studio.machine)
                    || studio.studio.error.is_some()
                    || studio.studio.next.is_some()
                    || studio.studio.ready_to_finish
            };
            let open = expanded_set(&studios, panel, current, body_height, &owed);
            for studio in studios {
                if open.contains(&studio.key()) {
                    let stage = stage_running(studio.machine);
                    let reach = reachable(studio.machine);
                    block_rows(&mut out, studio, panel, stage, reach);
                } else {
                    out.push(Row::Stacked(studio));
                }
            }
            // H4: with no `[hollywood]` the expanded panel's last row
            // names the setup command (R22).
            if panel.hollywood_hint {
                out.push(Row::SetupHint);
            }
        }
    }
    out
}
