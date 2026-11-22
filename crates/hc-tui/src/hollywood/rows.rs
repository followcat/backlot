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

use backlot_model::summary::{Exchange, RoleSummary, StudioSummary, Summary, Work};

use super::sheet::{Status, Wait, waiting_items};
use crate::panel::data::{MachineSummary, StudioRef};
use crate::studio::StudioKey;

/// A selected/clicked thing's stable identity.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Target {
    /// The `✦  H O L L Y W O O D` row — Enter focuses `bl-hollywood`.
    Title,
    /// A studio's frame or chip — Enter focuses its producer.
    Studio(StudioKey),
    /// The trailing `+ new studio` frame — Enter opens the wizard.
    NewStudio,
    /// Hollywood's newest headline — Enter reads its full report.
    Headline,
    /// A line of the user's exchange with Hollywood — Enter reads its
    /// full text or report body. Identity is the ledger seq.
    Talk(u64),
    /// A user-owned prompt row (studio, role) — `y`/`d` decide, `p`
    /// peeks, Enter focuses the role's terminal.
    Prompt(StudioKey, String),
    /// A machine whose stage is down (`offline == false`) or that is
    /// unreachable — `r` restarts the stage.
    Stage(Option<String>, bool),
    /// An open card row — `a` applies, `x` cancels, `s`/Enter shows.
    Card(String),
    /// A ready-to-finish studio — `f` finishes, Enter focuses it.
    Ready(StudioKey),
    /// The calm row — shown, not actionable.
    Calm,
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

/// One band row, borrowing its data from the documents.
pub(crate) enum Row<'a> {
    /// The newest `hollywood.reported` headline.
    Headline(&'a str),
    /// A waiting item — an index into the sheet's `waits`.
    Wait(usize),
    /// `✓ nothing waiting on you across <S> studios` — the calm row;
    /// `flags` is the open attention the sheet does not list (it is
    /// not the user's), so a `!N` chip never contradicts the row.
    Calm { studios: usize, flags: usize },
    /// A line of the user's exchange with Hollywood — the dialog's
    /// half drawn above the input (D21).
    Talk(&'a Exchange),
    /// `› <input>` — the `tell hollywood` line.
    Input,
    /// `✦ an agent can watch every studio …` — the bar without an
    /// agent (U7).
    Setup,
}

impl Row<'_> {
    pub fn target(&self, waits: &[Wait<'_>]) -> Target {
        match self {
            Row::Headline(_) => Target::Headline,
            Row::Talk(line) => Target::Talk(line.seq),
            Row::Wait(i) => waits[*i].target(),
            Row::Calm { .. } => Target::Calm,
            Row::Input => Target::Input,
            Row::Setup => Target::Setup,
        }
    }
}

/// The band's rows in draw order: the headline (with an agent), the
/// waiting items — or the calm row — then the input line, or the
/// setup line without an agent. The waits list is what `Row::Wait`
/// indexes and what scroll/detail bookkeeping numbers.
pub(crate) fn band_rows<'a>(
    machines: &'a [MachineSummary],
    status: &Status<'a>,
    agent: bool,
) -> (Vec<Wait<'a>>, Vec<Row<'a>>) {
    let mut out: Vec<Row<'a>> = Vec::new();
    let summary = local_summary(machines);
    if agent
        && let Some(headline) = summary
            .and_then(|s| s.hollywood.as_ref())
            .and_then(|h| h.headline.as_deref())
    {
        out.push(Row::Headline(headline));
    }
    let waits = waiting_items(machines, status);
    if waits.is_empty() {
        // Open attention no sheet row lists (each `attention_user`
        // role accounts for one record — the rest is the stage's or
        // a role's own flag): the chips still show it, so the calm
        // row does too.
        let flags: usize = machines
            .iter()
            .flat_map(|m| m.summary.studios.iter())
            .map(|s| {
                let mine = s.roles.iter().filter(|r| r.attention_user).count();
                (s.waiting as usize).saturating_sub(mine)
            })
            .sum();
        out.push(Row::Calm {
            studios: crate::panel::data::studio_count(machines),
            flags,
        });
    } else {
        out.extend((0..waits.len()).map(Row::Wait));
    }
    // The dialog's visible half: the newest exchange lines, oldest
    // first, right above the input they continue into.
    if agent {
        let talk = summary
            .and_then(|s| s.hollywood.as_ref())
            .map(|h| h.exchange.as_slice())
            .unwrap_or(&[]);
        out.extend(talk.iter().map(Row::Talk));
    }
    out.push(if agent { Row::Input } else { Row::Setup });
    (waits, out)
}

/// The setup line a bar without an agent shows: what an agent would
/// do for you, and the offer that opens its dialog (R22).
pub(crate) const SETUP_LEAD: &str =
    "✦ an agent can watch every studio and tell you what needs you:";
/// The offer the setup line names — Enter/click opens the in-TUI
/// dialog; `backlot hollywood setup` stays the CLI's own path.
pub(crate) const SETUP_COMMAND: &str = "⏎ hollywood setup";

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
