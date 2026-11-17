//! The panel's row model (U1): a pure `(machines, Panel, current,
//! body height)` → one-line rows map. Every row is a single line — a
//! block border, a person, a `├──┤` divider or a stacked block's
//! `╶ id …` line — so the scroll math stays in one-line steps.

use backlot_model::summary::{RoleSummary, StudioSummary, Work};

use super::data::{MachineSummary, StudioRef};
use super::{Panel, PanelMode};
use crate::chrome::palette::Palette;
use crate::chrome::status::{AgentStatus, status_color};
use crate::studio::StudioKey;
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
/// so anything the user can act on is named by machine plus document
/// ids, never by position.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Target {
    Title,
    /// A "stage not running" line — shown, not actionable.
    StageDown,
    /// The empty-state line — shown, not actionable.
    Empty,
    /// Any chrome row of an expanded block — top border, divider,
    /// error/next line or bottom. Enter/click makes the studio
    /// current.
    Studio(StudioKey),
    /// A stacked block's `╶ id …` line — Enter/click expands it.
    Block(StudioKey),
    /// A collapsed-strip row — Enter/click expands the panel.
    Strip(StudioKey),
    /// `(studio, role)` — a person row or one of its extra lines.
    Person(StudioKey, String),
    /// H4: the expanded panel's "no hollywood" hint — shown, not
    /// actionable.
    SetupHint,
}

/// An extra line under a person.
#[derive(Debug)]
pub(crate) enum PersonLine {
    /// `! <attention>` — the blocked colour.
    Attention,
    /// `! last turn failed: <e>` — only when no attention shows.
    Failed,
    /// The armed two-step reopen hint.
    Confirm,
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
    /// No `backlot` document anywhere or an unparseable one —
    /// "no studios".
    Empty,
    /// `╭ <id> ── !<n> ╮` — the studio id in the top border.
    BlockTop(StudioRef<'a>),
    /// `│ ! <error> │`
    BlockError(StudioRef<'a>),
    /// `│ → <next> │`
    BlockNext(StudioRef<'a>),
    /// `├──┤` between role groups.
    Divider(StudioRef<'a>),
    /// `╰──╯`
    BlockBottom(StudioRef<'a>),
    /// `│ <mark> <name> <task> <flag>   <state> │`
    Person {
        studio: StudioRef<'a>,
        role: &'a RoleSummary,
    },
    /// A person's attention/failed/confirm extra line.
    PersonLine {
        studio: StudioRef<'a>,
        role: &'a RoleSummary,
        kind: PersonLine,
    },
    /// A stacked block's single line: `╶ <id> ⠿2 ○3 ×1 !<n>`.
    Stacked(StudioRef<'a>),
    /// The collapsed strip's one line per studio.
    Strip(StudioRef<'a>),
    /// H4: `no hollywood — backlot hollywood setup …` — the expanded
    /// panel's last row while `[hollywood]` is absent (R22).
    SetupHint,
}

impl Row<'_> {
    /// The row's stable identity for selection, clicks and Enter.
    pub fn target(&self) -> Target {
        match self {
            Row::Title => Target::Title,
            Row::StageDown { .. } => Target::StageDown,
            Row::Empty => Target::Empty,
            Row::BlockTop(studio)
            | Row::BlockError(studio)
            | Row::BlockNext(studio)
            | Row::Divider(studio)
            | Row::BlockBottom(studio) => Target::Studio(studio.key()),
            Row::Person { studio, role } | Row::PersonLine { studio, role, .. } => {
                Target::Person(studio.key(), role.role.clone())
            }
            Row::Stacked(studio) => Target::Block(studio.key()),
            Row::Strip(studio) => Target::Strip(studio.key()),
            Row::SetupHint => Target::SetupHint,
        }
    }

    /// Rendered lines — every row is one line, so scroll metrics stay
    /// in line steps.
    pub fn height(&self) -> u16 {
        1
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

    /// The one-line block's state word (R17): `working`, `idle`,
    /// `blocked`, `lost` or `closed`.
    pub fn word(self) -> &'static str {
        match self {
            Mark::Working => "working",
            Mark::Waiting => "idle",
            Mark::Blocked => "blocked",
            Mark::Lost => "lost",
            Mark::Closed => "closed",
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

/// The mark order a stacked block's counts follow.
const MARK_ORDER: [Mark; 5] = [
    Mark::Working,
    Mark::Waiting,
    Mark::Blocked,
    Mark::Lost,
    Mark::Closed,
];

/// A stacked block's mark counts — `(mark, n)` for each nonzero mark
/// in `MARK_ORDER`.
pub(crate) fn mark_counts(studio: &StudioSummary) -> Vec<(Mark, usize)> {
    MARK_ORDER
        .into_iter()
        .filter_map(|mark| {
            let n = studio
                .roles
                .iter()
                .filter(|role| Mark::of(role) == mark)
                .count();
            (n > 0).then_some((mark, n))
        })
        .collect()
}

/// A person's line count: itself plus an attention-or-failed line and
/// the armed reopen hint.
fn person_height(studio: StudioRef<'_>, role: &RoleSummary, panel: &Panel) -> usize {
    1 + usize::from(role.attention.is_some() || role.failed.is_some())
        + usize::from(
            panel
                .reopen_confirm
                .as_ref()
                .is_some_and(|(s, r)| s == &studio.key() && r == &role.role),
        )
}

/// An expanded block's height: top + bottom borders, the `error` and
/// `next` lines when present, each non-empty group's people, and a
/// divider between groups.
fn block_height(studio: StudioRef<'_>, panel: &Panel) -> usize {
    let mut height =
        2 + usize::from(studio.studio.error.is_some()) + usize::from(studio.studio.next.is_some());
    let mut groups = 0usize;
    for group in GROUPS {
        let people = studio.studio.roles.iter().filter(|r| group_of(r) == group);
        let mut counted = false;
        for role in people {
            counted = true;
            height += person_height(studio, role, panel);
        }
        groups += usize::from(counted);
    }
    height + groups.saturating_sub(1)
}

/// Which studios draw an expanded block: all of them while every
/// expanded block fits the body; past that, the current studio and any
/// block `Space` opened by hand — `collapsed_blocks` wins either way.
fn expanded_set(
    studios: &[StudioRef<'_>],
    panel: &Panel,
    current: Option<&StudioKey>,
    body_height: u16,
) -> std::collections::BTreeSet<StudioKey> {
    // `body_height == 0` means the body was never drawn — no
    // constraint measured, so every block opens.
    let fits = body_height == 0
        || studios
            .iter()
            .map(|studio| block_height(*studio, panel))
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

/// One studio's expanded block: `╭ id ╮`, `error`/`next` lines, the
/// groups divided by `├──┤` rules — empty groups skipped — `╰──╯`.
fn block_rows<'a>(out: &mut Vec<Row<'a>>, studio: StudioRef<'a>, panel: &Panel) {
    out.push(Row::BlockTop(studio));
    if studio.studio.error.is_some() {
        out.push(Row::BlockError(studio));
    }
    if studio.studio.next.is_some() {
        out.push(Row::BlockNext(studio));
    }
    let mut first_group = true;
    for group in GROUPS {
        let people: Vec<&RoleSummary> = studio
            .studio
            .roles
            .iter()
            .filter(|r| group_of(r) == group)
            .collect();
        if people.is_empty() {
            continue;
        }
        if !first_group {
            out.push(Row::Divider(studio));
        }
        first_group = false;
        for role in people {
            let confirm = panel
                .reopen_confirm
                .as_ref()
                .is_some_and(|(s, r)| s == &studio.key() && r == &role.role);
            out.push(Row::Person { studio, role });
            if role.attention.is_some() {
                out.push(Row::PersonLine {
                    studio,
                    role,
                    kind: PersonLine::Attention,
                });
            } else if role.failed.is_some() {
                out.push(Row::PersonLine {
                    studio,
                    role,
                    kind: PersonLine::Failed,
                });
            }
            if confirm {
                out.push(Row::PersonLine {
                    studio,
                    role,
                    kind: PersonLine::Confirm,
                });
            }
        }
    }
    out.push(Row::BlockBottom(studio));
}

/// The whole row list for the current mode. An empty `machines` — no
/// machine's document parsed — is the "no studios" state, never an
/// error row. `stage_running` answers per machine (`None` is this
/// core); a machine that answered its document but runs no stage gets
/// its own line.
pub(crate) fn rows<'a>(
    machines: &'a [MachineSummary],
    stage_running: impl Fn(Option<&str>) -> bool,
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
        if !stage_running(machine.machine.as_deref()) {
            out.push(Row::StageDown {
                machine: machine.machine.as_deref(),
            });
        }
    }
    let studios = super::data::studios(machines);
    match panel.mode {
        PanelMode::Closed | PanelMode::Collapsed => {
            out.extend(studios.iter().map(|s| Row::Strip(*s)));
        }
        PanelMode::Expanded => {
            let open = expanded_set(&studios, panel, current, body_height);
            for studio in studios {
                if open.contains(&studio.key()) {
                    block_rows(&mut out, studio, panel);
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
