//! The panel's row model (U1): a pure `(machines, Panel, current,
//! body height)` → one-line rows map. Every row is a single line — a
//! block border, a person, a `├──┤` divider or a stacked block's
//! `╶ id …` line — so the scroll math stays in one-line steps.

use backlot_model::summary::{RoleSummary, StudioSummary};

use super::data::{MachineSummary, StudioRef};
use super::{Panel, PanelMode};
use crate::studio::StudioKey;
use crate::studio::glyphs::Person;

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
/// ids, never by position. Rows that only inform — the title,
/// dividers, block bottoms, spacers and state notes — carry `None`
/// and are skipped by the cursor keys and clicks alike (A2.2).
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Target {
    /// Shown, not actionable.
    None,
    /// Any header row of an expanded block — the top border, an
    /// error or a next line. Enter/click makes the studio current.
    Studio(StudioKey),
    /// A stacked block's `╶ id …` line — Enter/click expands it.
    Block(StudioKey),
    /// A collapsed-strip row — Enter/click expands the panel.
    Strip(StudioKey),
    /// `(studio, role)` — a person row or one of its extra lines.
    Person(StudioKey, String),
}

impl Target {
    /// The studio a row target belongs to — the selected block's
    /// accent border and the header's `▸` mark follow it (A2.3/A7.2).
    pub(crate) fn studio(&self) -> Option<&StudioKey> {
        match self {
            Target::Studio(key)
            | Target::Block(key)
            | Target::Strip(key)
            | Target::Person(key, _) => Some(key),
            Target::None => Option::None,
        }
    }
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
    /// Header rows inside a block share `Target::Studio`; dividers,
    /// bottoms and every non-row are `Target::None` (A2.2).
    pub fn target(&self) -> Target {
        match self {
            Row::Title
            | Row::StageDown { .. }
            | Row::Empty
            | Row::Divider(_)
            | Row::BlockBottom(_)
            | Row::SetupHint => Target::None,
            Row::BlockTop(studio) | Row::BlockError(studio) | Row::BlockNext(studio) => {
                Target::Studio(studio.key())
            }
            Row::Person { studio, role } | Row::PersonLine { studio, role, .. } => {
                Target::Person(studio.key(), role.role.clone())
            }
            Row::Stacked(studio) => Target::Block(studio.key()),
            Row::Strip(studio) => Target::Strip(studio.key()),
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
