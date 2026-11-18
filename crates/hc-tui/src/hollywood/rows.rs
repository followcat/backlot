//! The bar's model: one frame per studio — every machine's — plus
//! the trailing new-studio frame and the band rows under them. Pure
//! functions of the `backlot` documents — nothing is scraped from
//! Hollywood's terminal.
//!
//! Frame and row identities are document ids, never positions: the
//! frames rebuild on every publish and the selection has to survive.
//! A studio identity is `(machine, id)` — the same id on two
//! machines is two studios.

use ratatui::style::Color;

use backlot_model::summary::{RoleSummary, StudioSummary, Summary, Work};

use crate::panel::data::{MachineSummary, StudioRef};
use crate::studio::StudioKey;

/// A selected/clicked thing's stable identity.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Target {
    /// The `✦  H O L L Y W O O D` row — Enter focuses `bl-hollywood`.
    Title,
    /// The "stage not running" line — shown, not actionable.
    StageDown,
    /// A studio's frame — Enter focuses that studio's producer.
    Studio(StudioKey),
    /// The trailing `+ new studio` frame — Enter opens the wizard.
    NewStudio,
    /// Hollywood's newest headline — shown, not actionable.
    Headline,
    /// An open card row — `a` applies, `c` cancels, `s`/Enter shows.
    Card(String),
    /// The input line — `i`/Enter opens it for editing.
    Input,
    /// The no-agent bar's setup line — shown, not actionable.
    Setup,
}

/// One frame in the strip.
#[derive(Debug, Clone, Copy)]
pub(crate) enum Frame<'a> {
    /// A studio: `╭─ <id> ─…╮` with its state, the role on set, what
    /// it is doing and its cast — `machine` names it in the title
    /// once studios span machines.
    Studio(StudioRef<'a>),
    /// `+  new studio` — the wizard's frame, always last.
    New,
}

impl Frame<'_> {
    /// The frame's stable identity for selection, clicks and Enter.
    pub fn target(&self) -> Target {
        match self {
            Frame::Studio(studio) => Target::Studio(studio.key()),
            Frame::New => Target::NewStudio,
        }
    }
}

/// Every frame: the studios in aggregate order — this core's first,
/// then each added machine's — then the new-studio entry. A missing
/// document still offers the entry.
pub(crate) fn frames(machines: &[MachineSummary]) -> Vec<Frame<'_>> {
    let mut out: Vec<Frame<'_>> = crate::panel::data::studios(machines)
        .into_iter()
        .map(Frame::Studio)
        .collect();
    out.push(Frame::New);
    out
}

/// One band row, borrowing its data from the summary.
#[derive(Debug)]
pub(crate) enum Row<'a> {
    /// `stage not running` — same one-line read as the panel's.
    StageDown,
    /// The newest `hollywood.reported` headline.
    Headline(&'a str),
    /// `c<n> · <studio> · <action> — <why>` with its Apply/Cancel/Show
    /// targets.
    Card(&'a backlot_model::summary::CardSummary),
    /// `› <input>` — the `tell hollywood` line.
    Input,
    /// `✦ an agent can watch every studio …: backlot hollywood setup …`
    /// — the bar without an agent (U7).
    Setup,
}

impl Row<'_> {
    pub fn target(&self) -> Target {
        match self {
            Row::StageDown => Target::StageDown,
            Row::Headline(_) => Target::Headline,
            Row::Card(card) => Target::Card(card.id.clone()),
            Row::Input => Target::Input,
            Row::Setup => Target::Setup,
        }
    }
}

/// The band's rows in draw order: the stage line while the stage is
/// down, then (with an agent) the headline and the open cards, then
/// the input line — or, without an agent, the setup line in their
/// place.
pub(crate) fn band_rows<'a>(
    summary: Option<&'a Summary>,
    stage_running: bool,
    agent: bool,
) -> Vec<Row<'a>> {
    let mut out = Vec::new();
    if !stage_running {
        out.push(Row::StageDown);
    }
    if agent && let Some(hollywood) = summary.and_then(|s| s.hollywood.as_ref()) {
        if let Some(headline) = hollywood.headline.as_deref() {
            out.push(Row::Headline(headline));
        }
        out.extend(hollywood.cards.iter().map(Row::Card));
    }
    out.push(if agent { Row::Input } else { Row::Setup });
    out
}

/// The setup line a bar without an agent shows: what an agent would
/// do for you, and the command that writes its table.
pub(crate) const SETUP_LEAD: &str =
    "✦ an agent can watch every studio and tell you what needs you:";
/// The command the setup line names — `backlot hollywood setup`.
pub(crate) const SETUP_COMMAND: &str = "backlot hollywood setup --runner <runner> --model <model>";

/// This core's parsed document inside the aggregate — the headline,
/// the cards and the agent's state stay this core's.
pub(crate) fn local_summary(machines: &[MachineSummary]) -> Option<&Summary> {
    machines
        .iter()
        .find(|m| m.machine.is_none())
        .map(|m| &m.summary)
}

/// A role state's colour: live green, lost red, closed teal,
/// anything else dim — the band's agent square reads it.
pub(crate) fn state_color(state: &str, palette: &crate::chrome::palette::Palette) -> Color {
    match state {
        "live" => palette.green,
        "lost" => palette.red,
        "closed" => palette.teal,
        _ => palette.overlay0,
    }
}

/// The role that is on set now: the first working one, else the
/// producer, else `None`.
pub(crate) fn on_set(studio: &StudioSummary) -> Option<&RoleSummary> {
    studio
        .roles
        .iter()
        .find(|role| role.state == "live" && role.work == Some(Work::Working))
        .or_else(|| studio.roles.iter().find(|role| role.role == "producer"))
}
