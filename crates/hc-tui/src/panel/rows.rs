//! The panel's row model: a pure `(machines, Panel, current,
//! body height)` → one-line rows map. Every row is a single line — a
//! block border, a person, a task row, a cue or drawer line, or a
//! stacked block's `╶ id …` line — so the scroll math stays in
//! one-line steps.

use backlot_model::summary::{RoleSummary, StudioExchange, TaskSummary};

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
    /// A `stage not running` row — the bare machine line or a
    /// block's owed-step `◌ … — [r] start`; Enter/click is the
    /// `[r]` (U6c). `None` names this core.
    Stage(Option<String>),
    /// An expanded block's user-facing exchange row (`seq`) above `›` —
    /// Enter/click opens its full `messages/…` or `reports/…` file in
    /// the read-only `CardView`.
    Talk(StudioKey, u64),
    /// An expanded block's `›` line — Enter/click lands the keys on
    /// it: text is a `tell` to the producer, a leading `/` runs the
    /// deterministic verbs (`/stop`, `/pause t1`, …).
    Input(StudioKey),
    /// The `hollywood setup` hint row (R22) — Enter/click opens the
    /// in-TUI setup dialog.
    Setup,
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
            | Target::Fold(key)
            | Target::Talk(key, _)
            | Target::Input(key) => Some(key),
            Target::Stage(..) | Target::Setup | Target::None => Option::None,
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
    /// The studio's reported token/cost line (P4a) — emitted only
    /// while `usage` is `Some`, i.e. at least one counter moved.
    Usage {
        studio: StudioRef<'a>,
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
    /// One user-facing exchange turn (`›` user / `✦` producer or
    /// hollywood) above the studio's `›` input row.
    Talk {
        studio: StudioRef<'a>,
        line: &'a StudioExchange,
    },
    /// The block's `›` tell/command line — the studio's own input
    /// surface inside its frame, before the bottom border.
    Input {
        studio: StudioRef<'a>,
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
            // The stage-down rows are the `[r] start` their text
            // advertises (U6c); every other informative row stays
            // `None` — skipped by the cursor keys and clicks (A2.2).
            Row::StageDown { machine } => {
                Target::Stage(machine.map(std::borrow::ToOwned::to_owned))
            }
            Row::Owed {
                studio,
                kind: Owed::StageDown,
                ..
            } => Target::Stage(studio.machine.map(std::borrow::ToOwned::to_owned)),
            Row::Title
            | Row::Offline { .. }
            | Row::Empty
            | Row::Owed { .. }
            | Row::Usage { .. }
            | Row::BlockBottom(_)
            | Row::Drawer { .. } => Target::None,
            // R22: the hint is the dialog's button — actionable like
            // the Call Sheet's own setup line.
            Row::SetupHint => Target::Setup,
            Row::BlockTop { studio, .. } => Target::Studio(studio.key()),
            Row::Person { studio, role } => Target::Person(studio.key(), role.role.clone()),
            Row::PersonLine { studio, role, .. } => Target::Person(studio.key(), role.role.clone()),
            Row::Task { studio, task } => Target::Task(studio.key(), task.id.clone()),
            Row::AcceptedFold { studio, .. } => Target::Fold(studio.key()),
            Row::Talk { studio, line } => Target::Talk(studio.key(), line.seq),
            Row::Input { studio } => Target::Input(studio.key()),
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
/// roles, task rows, the accepted fold, closed roles, bounded
/// exchange rows, input row, bottom border.
fn block_rows<'a>(
    out: &mut Vec<Row<'a>>,
    studio: StudioRef<'a>,
    panel: &Panel,
    stage: bool,
    reachable: bool,
    talk_limit: usize,
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
    // P4a: runner-reported usage sits beside the owed line — a
    // factual count only while a delivery recorded any (D8).
    if studio.studio.usage.is_some() {
        out.push(Row::Usage { studio });
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
    if talk_limit > 0 {
        let ex = &studio.studio.exchange;
        let skip = ex.len().saturating_sub(talk_limit);
        for line in &ex[skip..] {
            out.push(Row::Talk { studio, line });
        }
    }
    // The `›` tell/command line closes the block — the studio's own
    // input surface inside its frame (S2 §4's input row).
    out.push(Row::Input { studio });
    out.push(Row::BlockBottom(studio));
}

/// The whole row list for `mode` — the drawn mode, which may differ
/// from `panel.mode`: an expanded dock that cannot fit beside the
/// panes resolves to the strip (`App::panel_mode_at`). An empty
/// `machines` — no machine's document parsed — is the "no studios"
/// state, never an error row. `stage_running` answers per machine
/// (`None` is this core); `reachable` answers whether the machine
/// itself is connected.
pub(crate) fn rows<'a>(
    machines: &'a [MachineSummary],
    stage_running: impl Fn(Option<&str>) -> bool,
    reachable: impl Fn(Option<&str>) -> bool,
    panel: &Panel,
    mode: PanelMode,
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
    match mode {
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
            let header_rows = out.len().saturating_sub(1);
            let owed = |studio: StudioRef<'_>| {
                !reachable(studio.machine)
                    || !stage_running(studio.machine)
                    || studio.studio.error.is_some()
                    || studio.studio.next.is_some()
                    || studio.studio.ready_to_finish
            };
            let open = expanded_set(&studios, panel, current, body_height, &owed);
            let budgets = talk_budgets(
                &studios,
                &open,
                panel,
                current,
                body_height,
                header_rows,
                &owed,
            );
            for studio in studios {
                let key = studio.key();
                if open.contains(&key) {
                    let stage = stage_running(studio.machine);
                    let reach = reachable(studio.machine);
                    let talk_limit = budgets.get(&key).copied().unwrap_or(0);
                    block_rows(&mut out, studio, panel, stage, reach, talk_limit);
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
