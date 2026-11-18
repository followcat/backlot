//! The panel's row model: a pure `(machines, Panel, current,
//! body height)` → one-line rows map. Every row is a single line — a
//! block border, a person, a task row, a cue or drawer line, or a
//! stacked block's `╶ id …` line — so the scroll math stays in
//! one-line steps.

use backlot_model::state::TaskStatus;
use backlot_model::summary::{RoleSummary, StudioSummary, TaskSummary};

use super::data::{MachineSummary, StudioRef};
use super::row_lines::{owed_text, person_drawer, task_drawer};
use super::{Panel, PanelMode};
use crate::studio::StudioKey;
use crate::studio::glyphs::Person;

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
    /// `closed — press o to reopen` after Enter on a closed role.
    ClosedHint,
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

/// The order a stacked block's mark counts follow — the urgents
/// first, then the working and quiet states.
const MARK_ORDER: [Person; 10] = [
    Person::UserPrompt,
    Person::DirPrompt,
    Person::Stalled,
    Person::Failed,
    Person::Lost,
    Person::Working,
    Person::Slot,
    Person::Idle,
    Person::Resumable,
    Person::Closed,
];

/// A stacked block's mark counts — `(mark, n)` for each nonzero mark
/// in `MARK_ORDER`.
pub(crate) fn mark_counts(studio: &StudioSummary) -> Vec<(Person, usize)> {
    MARK_ORDER
        .into_iter()
        .filter_map(|mark| {
            let n = studio
                .roles
                .iter()
                .filter(|role| Person::of(role) == mark)
                .count();
            (n > 0).then_some((mark, n))
        })
        .collect()
}

/// The health one studio's rows share — the block header's glyph
/// and the strip line's mark compute it the same way.
pub(crate) fn health_of(
    studio: &StudioSummary,
    stage: bool,
    reachable: bool,
) -> crate::studio::glyphs::Health {
    use crate::studio::glyphs::Health;
    if !reachable || studio.error.is_some() {
        Health::Offline
    } else if !stage {
        Health::StageDown
    } else if studio.ready_to_finish || studio.state == "finished" {
        Health::Ready
    } else {
        Health::Live
    }
}

/// The studio's non-closed roles in display order: `producer`,
/// `director`, `writer` first when present, then the actors in the
/// summary's order. Closed roles come after (S2 §2).
pub(crate) fn live_roles(studio: &StudioSummary) -> Vec<&RoleSummary> {
    let mut out: Vec<&RoleSummary> = Vec::new();
    for fixed in ["producer", "director", "writer"] {
        out.extend(
            studio
                .roles
                .iter()
                .filter(|r| r.role == fixed && r.state != "closed"),
        );
    }
    out.extend(studio.roles.iter().filter(|r| {
        r.state != "closed" && !matches!(r.role.as_str(), "producer" | "director" | "writer")
    }));
    out
}

/// The studio's closed roles — under the task rows (S2 §2).
fn closed_roles(studio: &StudioSummary) -> impl Iterator<Item = &RoleSummary> {
    studio.roles.iter().filter(|r| r.state == "closed")
}

/// Whether a live (non-closed) role's row already represents `task`.
fn represented(studio: &StudioSummary, task: &TaskSummary) -> bool {
    studio
        .roles
        .iter()
        .any(|r| r.state != "closed" && (r.task.as_deref() == Some(task.id.as_str())))
}

/// The dock's task rows: tasks with no live actor row, minus the
/// terminal states that never draw (`abandoned`, `taken_over`) and,
/// while the accepted fold is shut, `accepted`/`partially_accepted`.
/// Ledger order is the summary's order.
fn task_rows(studio: &StudioSummary, accepted_open: bool) -> impl Iterator<Item = &TaskSummary> {
    let accepted = studio
        .tasks
        .iter()
        .filter(|t| {
            matches!(
                t.status,
                TaskStatus::Accepted | TaskStatus::PartiallyAccepted
            )
        })
        .count();
    let fold = accepted >= 6 && !accepted_open;
    studio.tasks.iter().filter(move |t| {
        if matches!(t.status, TaskStatus::Abandoned | TaskStatus::TakenOver)
            || represented(studio, t)
        {
            return false;
        }
        if fold
            && matches!(
                t.status,
                TaskStatus::Accepted | TaskStatus::PartiallyAccepted
            )
        {
            return false;
        }
        true
    })
}

/// The number of accepted tasks behind the fold — the `✓ <n>`
/// line's count; `0` means no fold line at all.
fn fold_count(studio: &StudioSummary) -> usize {
    studio
        .tasks
        .iter()
        .filter(|t| {
            matches!(
                t.status,
                TaskStatus::Accepted | TaskStatus::PartiallyAccepted
            )
        })
        .filter(|t| !represented(studio, t))
        .count()
}

/// A person's line count: itself plus its cue/failed line, the
/// closed hint and an open drawer.
fn person_height(studio: StudioRef<'_>, role: &RoleSummary, panel: &Panel) -> usize {
    let mut height = 1 + usize::from(role.attention.is_some() || role.failed.is_some());
    height += usize::from(
        panel
            .closed_hint
            .as_ref()
            .is_some_and(|(s, r)| s == &studio.key() && r == &role.role),
    );
    height += usize::from(
        panel
            .drawer
            .as_ref()
            .is_some_and(|d| *d == Target::Person(studio.key(), role.role.clone())),
    ) * 3;
    height
}

/// The task section's line count: task rows plus the fold line and
/// any open drawer under a task row.
fn tasks_height(studio: StudioRef<'_>, panel: &Panel, accepted_open: bool) -> usize {
    let mut height = task_rows(studio.studio, accepted_open)
        .map(|t| {
            1 + usize::from(
                panel
                    .drawer
                    .as_ref()
                    .is_some_and(|d| *d == Target::Task(studio.key(), t.id.clone())),
            ) * 3
        })
        .sum::<usize>();
    height += usize::from(fold_count(studio.studio) >= 6);
    height
}

/// An expanded block's height: top + bottom borders, the owed-step
/// line when present, the armed finish confirm, each person's
/// lines, the task section and the closed section.
fn block_height(studio: StudioRef<'_>, panel: &Panel, owed: bool, accepted_open: bool) -> usize {
    let mut height =
        2 + usize::from(owed) + usize::from(panel.finish_confirm.as_ref() == Some(&studio.key()));
    for role in live_roles(studio.studio) {
        height += person_height(studio, role, panel);
    }
    height += tasks_height(studio, panel, accepted_open);
    for role in closed_roles(studio.studio) {
        height += person_height(studio, role, panel);
    }
    height
}

/// Which studios draw an expanded block: all of them while every
/// expanded block fits the body; past that, the current studio and any
/// block `Space` opened by hand — `collapsed_blocks` wins either way.
fn expanded_set(
    studios: &[StudioRef<'_>],
    panel: &Panel,
    current: Option<&StudioKey>,
    body_height: u16,
    ctx: &dyn Fn(StudioRef<'_>) -> bool,
) -> std::collections::BTreeSet<StudioKey> {
    let fits = body_height == 0
        || studios
            .iter()
            .map(|studio| {
                block_height(
                    *studio,
                    panel,
                    ctx(*studio),
                    panel.accepted_open(&studio.key()),
                )
            })
            .sum::<usize>()
            <= body_height as usize;
    studios
        .iter()
        .filter_map(|studio| {
            let key = studio.key();
            let open = if panel.expanded_blocks.contains(&key) {
                true
            } else if panel.collapsed_blocks.contains(&key) {
                false
            } else {
                fits || current == Some(&key)
            };
            open.then_some(key)
        })
        .collect()
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
        if panel
            .closed_hint
            .as_ref()
            .is_some_and(|(s, r)| s == &key && r == &role.role)
        {
            out.push(Row::PersonLine {
                studio,
                role,
                kind: PersonLine::ClosedHint,
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

/// The collapsed strip's role lines: live roles that are working or
/// carry an attention — the strip is never blank while the studio
/// has something to say (A4.3).
fn strip_roles(studio: &StudioSummary) -> Vec<&RoleSummary> {
    studio
        .roles
        .iter()
        .filter(|r| {
            r.state != "closed"
                && (r.attention.is_some()
                    || r.work == Some(backlot_model::summary::Work::Working)
                    || r.work == Some(backlot_model::summary::Work::Blocked))
        })
        .collect()
}
