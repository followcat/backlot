//! The panel's row model: a pure `(machines, Panel, current,
//! body height)` → one-line rows map. Every row is a single line — a
//! block border, a person, a task row, a cue or drawer line, or a
//! stacked block's `╶ id …` line — so the scroll math stays in
//! one-line steps.

use backlot_model::summary::{
    OutwardCardSummary, RoleSummary, StudioExchange, StudioSummary, TaskSummary,
};

use super::data::{MachineSummary, StudioRef};
use super::row_lines::{owed_text, person_drawer, task_drawer};
use super::rows_fit::*;
use super::rows_fold::{BlockState, CompactKind, block_states};
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
    /// `(studio, card id)` — an open outward push/PR card row.
    OutwardCard(StudioKey, String),
    /// Active opening-design block (`◆ design · R<r> · ...`) inside a studio card.
    Design(StudioKey),
    /// The `✓ <n> done · <ids>` fold line — Space/Enter opens it.
    Fold(StudioKey),
    /// The `□ <n> closed · <names>` fold line — Space/Enter opens it.
    ClosedFold(StudioKey),
    /// A `stage stopped` row — the bare machine line or a block's
    /// owed-step `◌ … · s start`; Enter/click is the `s` (U6c).
    /// `None` names this core.
    Stage(Option<String>),
    /// An expanded block's user-facing exchange row (`seq`) above `›` —
    /// Enter/click opens its full `messages/…` or `reports/…` file in
    /// the read-only `CardView`.
    Talk(StudioKey, u64),
    /// An expanded block's `›` line — Enter/click lands the keys on
    /// it: text is a `tell` to the producer, a leading `/` runs the
    /// deterministic verbs (`/stop`, `/pause t1`, …).
    Input(StudioKey),
    /// A finished studio's `↺ reopen producer [o]` line — `o` runs
    /// `backlot reopen --person producer`; Enter never respawns.
    ReopenProducer(StudioKey),
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
            | Target::OutwardCard(key, _)
            | Target::Design(key)
            | Target::Fold(key)
            | Target::ClosedFold(key)
            | Target::Talk(key, _)
            | Target::Input(key)
            | Target::ReopenProducer(key) => Some(key),
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
    /// `· report pending` — the wrap report is still owed (R3-3).
    ReportPending,
    /// `! finish <id>? [y]` — `f` armed the inline confirm.
    Confirm,
    /// `◌ stage stopped · s start`.
    StageDown,
    /// `◌ <start command>` when server is stopped (read-only).
    ServerStopped,
    /// `× <machine> unreachable`.
    Offline,
    /// `! <error>` — the ledger/stage error.
    Error,
    /// `! upgrade backlot` — ledger holds newer unknown kinds.
    Upgrade,
    /// `· next: <step>` — the studio's own next step.
    Next,
}

/// One display row, borrowing its data from the summaries.
#[derive(Debug)]
pub(crate) enum Row<'a> {
    Title,
    /// `◌ stage stopped · s start` — one per machine whose document
    /// answered but whose stage is down; `machine` names it when
    /// several machines answered.
    StageDown {
        machine: Option<&'a str>,
    },
    /// Server terminal is gone while `backlot-cat` is present — read-only
    /// command hint, never starts anything itself.
    ServerStopped {
        cmd: String,
    },
    /// A listed machine that is not connected; its studios cannot
    /// answer.
    Offline {
        machine: &'a str,
    },
    /// W1b: no published studios, but a stopped machine's ledger lists
    /// some — `<n> studios on stopped stages — s to start`. `machine`
    /// names the one machine holding them (`Some(None)` is this core)
    /// when only one does, so `s` can start that stage; `None` with
    /// several, where no single stage is the row's.
    StoppedLedger {
        count: usize,
        machine: Option<Option<String>>,
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
    /// 1 blank row separating studio blocks or compact-rows groups.
    Spacer,
    /// Borderless 1-line compact studio (`✓ gamma-web  5/5 ready [f]`).
    CompactStudio {
        studio: StudioRef<'a>,
        kind: CompactKind,
    },
    /// `│ <mark> <role_badge>[ <flag>]  <state>[ <age>][ · <detail…>] │`
    Person {
        studio: StudioRef<'a>,
        role: &'a RoleSummary,
    },
    /// `├───┤` — the thin rule between two non-empty role groups.
    Divider {
        studio: StudioRef<'a>,
    },
    /// A task with no live actor row: `│ · task t2 (waits t1)  dep │`.
    Task {
        studio: StudioRef<'a>,
        task: &'a TaskSummary,
    },
    /// An open outward push/PR card inside a studio block.
    OutwardCard {
        studio: StudioRef<'a>,
        card: &'a OutwardCardSummary,
    },
    /// Active opening-design header (`◆ design · R<r> · you|proposal|review|summary`).
    DesignHeader {
        studio: StudioRef<'a>,
        design: &'a backlot_model::summary::DesignSummary,
    },
    /// Up to 3 opening-design question or summary preview rows.
    DesignPreview {
        studio: StudioRef<'a>,
        text: String,
    },
    /// Opening-design action footer (`r reply  o read  a sign` or `discuss/skip`).
    DesignActions {
        studio: StudioRef<'a>,
        suggest_skip: bool,
    },
    /// `✓ <n> done · <ids>` — the fold line when 2+ tasks are done.
    AcceptedFold {
        studio: StudioRef<'a>,
        open: bool,
    },
    /// `□ <n> closed · <names>` — the fold line when closed roles exist.
    ClosedFold {
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
    /// Delivery status line (`… sending…`, `… queued · <reason>`,
    /// `✓ delivered · <age>`, `⠶ thinking…`) directly under the newest user tell.
    TalkStatus {
        studio: StudioRef<'a>,
        line: &'a StudioExchange,
    },
    /// The `├──┤` rule separating open roles/tasks/talk from the studio's
    /// `›` compose field.
    InputDivider {
        studio: StudioRef<'a>,
    },
    /// The block's `›` tell/command line — the studio's own input
    /// surface inside its frame, before the bottom border.
    Input {
        studio: StudioRef<'a>,
    },
    /// A finished studio's `↺ reopen producer [o]` line where the
    /// `›` line would sit — selectable, its action is `o` (R3-N7).
    ReopenProducer {
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
    /// R3-N9: compact 1-line calm/owed summary under a strip studio
    /// when it has zero active/blocked `StripRole` lines.
    StripSummary {
        studio: StudioRef<'a>,
        stage: bool,
        reachable: bool,
    },
    /// H4: `no hollywood — backlot hollywood setup …` — the expanded
    /// panel's last row while `[hollywood]` is absent (R22).
    SetupHint,
    /// S2: `backlot <v> installed — restart the client to update …`
    /// at the foot of the lot when a newer install was found.
    Update {
        text: String,
    },
}

impl Row<'_> {
    /// The row's stable identity for selection, clicks and Enter.
    /// Owed/cue/drawer/border rows are `Target::None` (A2.2).
    pub fn target(&self) -> Target {
        match self {
            // The stage-down rows are the `s start` their text
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
            // W1b: the stopped-ledger line starts the one stage it names.
            Row::StoppedLedger {
                machine: Some(machine),
                ..
            } => Target::Stage(machine.clone()),
            Row::Title
            | Row::ServerStopped { .. }
            | Row::StoppedLedger { .. }
            | Row::Offline { .. }
            | Row::Empty
            | Row::Owed { .. }
            | Row::Usage { .. }
            | Row::InputDivider { .. }
            | Row::BlockBottom(_)
            | Row::Spacer
            | Row::Divider { .. }
            | Row::Drawer { .. }
            | Row::Update { .. } => Target::None,
            // R22: the hint is the dialog's button — actionable like
            // the Call Sheet's own setup line.
            Row::SetupHint => Target::Setup,
            Row::BlockTop { studio, .. } => Target::Studio(studio.key()),
            Row::CompactStudio { studio, .. } => Target::Block(studio.key()),
            Row::Person { studio, role } => Target::Person(studio.key(), role.role.clone()),
            Row::Task { studio, task } => Target::Task(studio.key(), task.id.clone()),
            Row::OutwardCard { studio, card } => Target::OutwardCard(studio.key(), card.id.clone()),
            Row::DesignHeader { studio, .. }
            | Row::DesignPreview { studio, .. }
            | Row::DesignActions { studio, .. } => Target::Design(studio.key()),
            Row::AcceptedFold { studio, .. } => Target::Fold(studio.key()),
            Row::ClosedFold { studio, .. } => Target::ClosedFold(studio.key()),
            Row::Talk { studio, line } => Target::Talk(studio.key(), line.seq),
            Row::TalkStatus { .. } => Target::None,
            Row::Input { studio } => Target::Input(studio.key()),
            Row::ReopenProducer { studio } => Target::ReopenProducer(studio.key()),
            Row::Stacked(studio) => Target::Block(studio.key()),
            Row::Strip { studio, .. }
            | Row::StripRole { studio, .. }
            | Row::StripSummary { studio, .. } => Target::Strip(studio.key()),
        }
    }

    /// Rendered lines — every row is one line, so scroll metrics stay
    /// in line steps.
    pub fn height(&self) -> u16 {
        1
    }

    /// Studio owning this rendered row (including chrome/divider/owed lines
    /// whose [`Target`] is [`Target::None`]).
    pub(crate) fn owning_studio(&self) -> Option<StudioKey> {
        match self {
            Row::BlockTop { studio, .. }
            | Row::Owed { studio, .. }
            | Row::Usage { studio }
            | Row::BlockBottom(studio)
            | Row::Person { studio, .. }
            | Row::Divider { studio }
            | Row::CompactStudio { studio, .. }
            | Row::Task { studio, .. }
            | Row::OutwardCard { studio, .. }
            | Row::DesignHeader { studio, .. }
            | Row::DesignPreview { studio, .. }
            | Row::DesignActions { studio, .. }
            | Row::AcceptedFold { studio, .. }
            | Row::ClosedFold { studio, .. }
            | Row::Drawer { studio, .. }
            | Row::Talk { studio, .. }
            | Row::TalkStatus { studio, .. }
            | Row::InputDivider { studio }
            | Row::Input { studio }
            | Row::ReopenProducer { studio }
            | Row::Stacked(studio)
            | Row::Strip { studio, .. }
            | Row::StripRole { studio, .. }
            | Row::StripSummary { studio, .. } => Option::Some(studio.key()),
            Row::Title
            | Row::Spacer
            | Row::StageDown { .. }
            | Row::ServerStopped { .. }
            | Row::Empty
            | Row::StoppedLedger { .. }
            | Row::Offline { .. }
            | Row::SetupHint
            | Row::Update { .. } => Option::None,
        }
    }

    /// Which studio block and section an actionable row belongs to, for
    /// `Tab` / `Shift+Tab` section cycling inside the focused Lot.
    pub(crate) fn section_key(&self) -> Option<(StudioKey, Section)> {
        if self.target() == Target::None {
            return None;
        }
        match self {
            Row::BlockTop { studio, .. }
            | Row::Owed {
                studio,
                kind: Owed::StageDown,
                ..
            }
            | Row::CompactStudio { studio, .. }
            | Row::Stacked(studio)
            | Row::Strip { studio, .. }
            | Row::StripRole { studio, .. }
            | Row::StripSummary { studio, .. } => Some((studio.key(), Section::Header)),
            Row::Person { studio, role } if role.state != "closed" => {
                Some((studio.key(), Section::People))
            }
            Row::Person { studio, .. }
            | Row::OutwardCard { studio, .. }
            | Row::Task { studio, .. }
            | Row::DesignHeader { studio, .. }
            | Row::DesignPreview { studio, .. }
            | Row::DesignActions { studio, .. }
            | Row::AcceptedFold { studio, .. }
            | Row::ClosedFold { studio, .. } => Some((studio.key(), Section::Tasks)),
            Row::Talk { studio, .. } => Some((studio.key(), Section::Exchange)),
            Row::Input { studio } | Row::ReopenProducer { studio } => {
                Some((studio.key(), Section::Input))
            }
            Row::Title
            | Row::StageDown { .. }
            | Row::ServerStopped { .. }
            | Row::Offline { .. }
            | Row::StoppedLedger { .. }
            | Row::Empty
            | Row::Owed { .. }
            | Row::Usage { .. }
            | Row::TalkStatus { .. }
            | Row::InputDivider { .. }
            | Row::BlockBottom(_)
            | Row::Spacer
            | Row::Divider { .. }
            | Row::Drawer { .. }
            | Row::SetupHint
            | Row::Update { .. } => None,
        }
    }
}

/// Ordered sections inside a Lot studio block (`Tab` / `Shift+Tab` stops).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) enum Section {
    Header,
    People,
    Tasks,
    Exchange,
    Input,
}

/// One studio's expanded block: header, owed-step subheader, live
/// roles, task rows, folds, bounded exchange rows, input row, bottom border.
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
        if studio.machine.is_none()
            && let Some(cmd) = panel.server_stopped_cmd.as_deref()
        {
            Some((Owed::ServerStopped, cmd.to_owned()))
        } else {
            Some((Owed::StageDown, "stage stopped · s start".to_string()))
        }
    } else if let Some(error) = &studio.studio.error {
        Some((Owed::Error, error.clone()))
    } else if studio.studio.needs_upgrade {
        Some((Owed::Upgrade, upgrade_text(studio.studio)))
    } else if studio.studio.ready_to_finish {
        // R3-3: `f` is offered as soon as every task is accepted —
        // a post-acceptance `node report` step must not hide it.
        Some((Owed::Finish, "finish studio · user [f]".to_string()))
    } else {
        studio
            .studio
            .next
            .as_ref()
            .map(|next| (Owed::Step, owed_text(studio.studio, next)))
    };
    if let Some((kind, text)) = owed {
        out.push(Row::Owed { studio, kind, text });
    }
    // R3-3: while the wrap report is missing the offer carries a
    // dim `report pending` under it.
    let report_pending = !studio.studio.needs_upgrade && super::data::report_pending(studio.studio);
    if report_pending {
        out.push(Row::Owed {
            studio,
            kind: Owed::ReportPending,
            text: "report pending".to_string(),
        });
    }
    // `f` armed the finish confirm — its `y`/`n` line sits under
    // the owed step, cleared by Esc or a selection move (S2 §2). An
    // early finish (report still pending) warns inside the ask.
    if !studio.studio.needs_upgrade && panel.finish_confirm.as_ref() == Some(&studio.key()) {
        let pending = if report_pending {
            " (report pending)"
        } else {
            ""
        };
        out.push(Row::Owed {
            studio,
            kind: Owed::Confirm,
            text: format!("finish {}{pending}? [y/n]", studio.studio.id),
        });
    }
    // The studio's own next step, when its state names one — the
    // ledger's owed step is a different line above it.
    if !studio.studio.needs_upgrade
        && let Some(step) = super::data::next_step(studio.studio)
    {
        out.push(Row::Owed {
            studio,
            kind: Owed::Next,
            text: format!("next: {step}"),
        });
    }
    // P4a: runner-reported usage sits beside the owed line — a
    // factual count only while a delivery recorded any (D8).
    if studio.studio.usage.is_some() {
        out.push(Row::Usage { studio });
    }
    super::draw_design::push_block_rows(out, studio);
    let emit_person = |out: &mut Vec<Row<'a>>, role: &'a RoleSummary| {
        let key = studio.key();
        out.push(Row::Person { studio, role });
        let target = Target::Person(key, role.role.clone());
        if panel.drawer.as_ref() == Some(&target) {
            for text in person_drawer(studio, role) {
                out.push(Row::Drawer { studio, text });
            }
        }
    };
    let shown_roles = live_roles(studio.studio);
    push_grouped_roles(out, studio, &shown_roles, emit_person);
    for card in &studio.studio.outward_cards {
        out.push(Row::OutwardCard { studio, card });
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
    if folds >= 2 {
        out.push(Row::AcceptedFold {
            studio,
            open: accepted_open,
        });
    }
    if closed_roles(studio.studio).next().is_some() {
        let closed_open = panel.closed_open(&studio.key());
        out.push(Row::ClosedFold {
            studio,
            open: closed_open,
        });
        if closed_open {
            for role in closed_roles(studio.studio) {
                emit_person(out, role);
            }
        }
    }
    if talk_limit > 0 {
        let ex = &studio.studio.exchange;
        let last_user = ex.last().filter(|l| l.who == "user");
        let show_status = last_user.is_some() && talk_limit >= 2;
        let turns = if show_status {
            talk_limit.saturating_sub(1)
        } else {
            talk_limit
        };
        let skip = ex.len().saturating_sub(turns);
        for line in &ex[skip..] {
            out.push(Row::Talk { studio, line });
        }
        if show_status && let Some(line) = last_user {
            out.push(Row::TalkStatus { studio, line });
        }
    }
    // The `›` tell/command line closes the block — the studio's own
    // input surface inside its frame (S2 §4's input row). A finished
    // studio has no producer to tell: its `↺ reopen producer [o]`
    // line is the row's resting action instead (R3-N7). `state` reads
    // `finished` one publish early — `ready_to_finish` is the
    // discriminator `StudioMenu::studio` uses too.
    if studio.studio.state == "finished" && !studio.studio.ready_to_finish {
        out.push(Row::ReopenProducer { studio });
    } else {
        out.push(Row::InputDivider { studio });
        out.push(Row::Input { studio });
    }
    out.push(Row::BlockBottom(studio));
}

fn upgrade_text(studio: &StudioSummary) -> String {
    if studio.unknown_kinds.is_empty() {
        "upgrade backlot".to_string()
    } else {
        format!("upgrade backlot · {}", studio.unknown_kinds.join(", "))
    }
}

/// Emit `roles` ordered by `Producer` / `Director` / `Writer` / `Actors`,
/// placing a `Row::Divider` rule between each pair of non-empty groups.
fn push_grouped_roles<'a>(
    out: &mut Vec<Row<'a>>,
    studio: StudioRef<'a>,
    roles: &[&'a RoleSummary],
    mut emit_role: impl FnMut(&mut Vec<Row<'a>>, &'a RoleSummary),
) {
    let mut first = true;
    for group in [
        RoleGroup::Producer,
        RoleGroup::Director,
        RoleGroup::Writer,
        RoleGroup::Actors,
    ] {
        let group_roles: Vec<&RoleSummary> = roles
            .iter()
            .copied()
            .filter(|role| role_group(&role.role) == group)
            .collect();
        if group_roles.is_empty() {
            continue;
        }
        if !first {
            out.push(Row::Divider { studio });
        }
        first = false;
        for role in group_roles {
            emit_role(out, role);
        }
    }
}

/// An unselected active studio's folded block: header, one `Person` row per
/// active/alert live role separated by role-group dividers, and bottom border.
fn folded_block_rows<'a>(
    out: &mut Vec<Row<'a>>,
    studio: StudioRef<'a>,
    stage: bool,
    reachable: bool,
) {
    let health = health_of(studio.studio, stage, reachable);
    out.push(Row::BlockTop { studio, health });
    if studio.studio.needs_upgrade {
        out.push(Row::Owed {
            studio,
            kind: Owed::Upgrade,
            text: upgrade_text(studio.studio),
        });
    }
    super::draw_design::push_folded_rows(out, studio);
    let folded = super::rows_fold::folded_roles(studio.studio);
    push_grouped_roles(out, studio, &folded, |out, role| {
        out.push(Row::Person { studio, role });
    });
    out.push(Row::BlockBottom(studio));
}

/// W1b: what the panel's empty state reads when no document has a
/// studio but a stopped machine's ledger lists some. `machine` is
/// `Some` only when one machine holds them all, so `s` can start that
/// one stage (`None` inside the outer `Some` is this core).
#[derive(Debug, Clone)]
pub(crate) struct StoppedLedger {
    pub count: usize,
    pub machine: Option<Option<String>>,
}

/// The whole row list for `mode` — the drawn mode, which may differ
/// from `panel.mode`: an expanded dock that cannot fit beside the
/// panes resolves to the strip (`App::panel_mode_at`). An empty
/// `machines` — no machine's document parsed — is the "no studios"
/// state, never an error row. `stage_running` answers per machine
/// (`None` is this core); `reachable` answers whether the machine
/// itself is connected. `stopped` replaces the bare "no studios"
/// line while a stopped stage's ledger lists studios (W1b).
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
            if let Some(cmd) = panel.server_stopped_cmd.as_deref() {
                out.push(Row::ServerStopped {
                    cmd: cmd.to_owned(),
                });
            } else {
                out.push(Row::StageDown { machine: None });
            }
        }
        match &panel.stopped {
            Some(stopped) => out.push(Row::StoppedLedger {
                count: stopped.count,
                machine: stopped.machine.clone(),
            }),
            None => out.push(Row::Empty),
        }
        // R3-M5: a brand-new install is exactly who the setup hint
        // is for — the early return must not swallow it.
        if mode == PanelMode::Expanded && panel.hollywood_hint {
            out.push(Row::SetupHint);
        }
        push_update(&mut out, panel);
        return out;
    }
    for machine in machines {
        // A machine's own line only when it has no studio blocks to
        // carry the state — inside a block the owed-step subheader
        // says the same thing (`◌ stage stopped`, `× …unreachable`).
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
            if machine.machine.is_none()
                && let Some(cmd) = panel.server_stopped_cmd.as_deref()
            {
                out.push(Row::ServerStopped {
                    cmd: cmd.to_owned(),
                });
            } else {
                out.push(Row::StageDown {
                    machine: machine.machine.as_deref(),
                });
            }
        }
    }
    let studios = super::data::studios(machines);
    match mode {
        PanelMode::Closed | PanelMode::Collapsed => {
            for studio in &studios {
                let stage = stage_running(studio.machine);
                let reach = reachable(studio.machine);
                let health = health_of(studio.studio, stage, reach);
                out.push(Row::Strip {
                    studio: *studio,
                    health,
                });
                let roles = strip_roles(studio.studio);
                if studio.studio.needs_upgrade
                    || (roles.is_empty() && (!stage || !reach || studio.studio.error.is_some()))
                {
                    out.push(Row::StripSummary {
                        studio: *studio,
                        stage,
                        reachable: reach,
                    });
                }
                for role in roles {
                    out.push(Row::StripRole {
                        studio: *studio,
                        role,
                    });
                }
            }
        }
        PanelMode::Expanded => {
            let header_rows = out.len().saturating_sub(1);
            let compact_of = |studio: StudioRef<'_>| {
                super::draw_compact::compact_kind(
                    studio.studio,
                    stage_running(studio.machine),
                    reachable(studio.machine),
                )
            };
            let owed = |studio: StudioRef<'_>| {
                !reachable(studio.machine)
                    || !stage_running(studio.machine)
                    || studio.studio.error.is_some()
                    || studio.studio.needs_upgrade
                    || studio.studio.next.is_some()
                    || studio.studio.ready_to_finish
            };
            let states = block_states(&studios, panel, current, body_height, &compact_of, &owed);
            let budgets = talk_budgets(
                &studios,
                &states,
                panel,
                current,
                body_height,
                header_rows,
                &owed,
            );
            let state_of = |studio: StudioRef<'_>| {
                states
                    .get(&studio.key())
                    .copied()
                    .unwrap_or(BlockState::Stacked)
            };
            let is_single_row =
                |s: BlockState| matches!(s, BlockState::Compact(_) | BlockState::Stacked);
            let spacer_slots: Vec<bool> = (0..studios.len())
                .map(|i| {
                    let Some(next) = studios.get(i + 1) else {
                        return false;
                    };
                    !(is_single_row(state_of(studios[i])) && is_single_row(state_of(*next)))
                })
                .collect();
            let spacer_count = spacer_slots.iter().filter(|&&emit| emit).count();
            let mut body_blocks = Vec::new();
            let mut end_indices = Vec::with_capacity(studios.len());
            for studio in studios {
                let key = studio.key();
                let stage = stage_running(studio.machine);
                let reach = reachable(studio.machine);
                match state_of(studio) {
                    BlockState::Full => {
                        let talk_limit = budgets.get(&key).copied().unwrap_or(0);
                        block_rows(&mut body_blocks, studio, panel, stage, reach, talk_limit);
                    }
                    BlockState::Folded => {
                        folded_block_rows(&mut body_blocks, studio, stage, reach);
                    }
                    BlockState::Compact(kind) => {
                        body_blocks.push(Row::CompactStudio { studio, kind });
                        if panel.finish_confirm.as_ref() == Some(&key) {
                            let pending = if super::data::report_pending(studio.studio) {
                                " (report pending)"
                            } else {
                                ""
                            };
                            body_blocks.push(Row::Owed {
                                studio,
                                kind: Owed::Confirm,
                                text: format!("finish {}{pending}? [y/n]", studio.studio.id),
                            });
                        }
                    }
                    BlockState::Stacked => body_blocks.push(Row::Stacked(studio)),
                }
                end_indices.push(body_blocks.len());
            }
            let tail_chrome =
                usize::from(panel.hollywood_hint) + usize::from(panel.update.is_some());
            let include_spacers = body_height == 0
                || header_rows + body_blocks.len() + tail_chrome + spacer_count
                    <= usize::from(body_height);
            let mut start = 0usize;
            let mut iter = body_blocks.into_iter();
            for (i, end) in end_indices.into_iter().enumerate() {
                let take = end - start;
                out.extend(iter.by_ref().take(take));
                start = end;
                if include_spacers && spacer_slots[i] {
                    out.push(Row::Spacer);
                }
            }
            // H4: with no `[hollywood]` the expanded panel's last row
            // names the setup command (R22).
            if panel.hollywood_hint {
                out.push(Row::SetupHint);
            }
        }
    }
    push_update(&mut out, panel);
    out
}

/// S2: the newer-install line closes the lot — one dim row after the
/// studios, whatever the mode.
fn push_update<'a>(out: &mut Vec<Row<'a>>, panel: &Panel) {
    if let Some(hint) = &panel.update {
        out.push(Row::Update { text: hint.text() });
    }
}
