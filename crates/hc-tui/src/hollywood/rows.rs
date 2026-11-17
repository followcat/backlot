//! The bar's model: one frame per studio plus the trailing new-studio
//! frame, the band rows under them, and the collapsed strip's
//! content. Pure functions of the `backlot` document — nothing is
//! scraped from Hollywood's terminal.
//!
//! Frame and row identities are document ids, never positions: the
//! frames rebuild on every publish and the selection has to survive.

use backlot_model::summary::{RoleSummary, StudioSummary, Summary, Work};

/// A selected/clicked thing's stable identity.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Target {
    /// The `✦  H O L L Y W O O D` row — Enter focuses `bl-hollywood`.
    Title,
    /// The "stage not running" line — shown, not actionable.
    StageDown,
    /// A studio's frame — Enter focuses that studio's producer.
    Studio(String),
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
    /// it is doing and its cast.
    Studio(&'a StudioSummary),
    /// `+  new studio` — the wizard's frame, always last.
    New,
}

impl Frame<'_> {
    /// The frame's stable identity for selection, clicks and Enter.
    pub fn target(&self) -> Target {
        match self {
            Frame::Studio(studio) => Target::Studio(studio.id.clone()),
            Frame::New => Target::NewStudio,
        }
    }
}

/// Every frame: the studios in document order, then the new-studio
/// entry. A missing document still offers the entry.
pub(crate) fn frames(summary: Option<&Summary>) -> Vec<Frame<'_>> {
    let mut out: Vec<Frame<'_>> = match summary {
        Some(summary) => summary.studios.iter().map(Frame::Studio).collect(),
        None => Vec::new(),
    };
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

/// The collapsed strip's content: the agent's role state (`None`
/// without an agent, drawn dim), how many studios there are, and how
/// many things wait on you.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Strip<'a> {
    pub state: Option<&'a str>,
    pub studios: usize,
    /// The studios' open attentions, summed.
    pub waiting: u32,
}

/// Read the strip's content out of the document.
pub(crate) fn strip(summary: Option<&Summary>, agent: bool) -> Strip<'_> {
    let (studios, hollywood) = match summary {
        Some(summary) => (summary.studios.as_slice(), summary.hollywood.as_ref()),
        None => (&[][..], None),
    };
    Strip {
        state: agent.then(|| hollywood.map(|h| h.state.as_str()).unwrap_or("down")),
        studios: studios.len(),
        waiting: studios.iter().map(|s| s.waiting).sum(),
    }
}

/// The strip's plain text, in draw order — the same text the
/// renderer writes, piece for piece. The tests read it; the renderer
/// builds the coloured pieces itself.
#[cfg(test)]
pub(crate) fn strip_text(summary: Option<&Summary>, agent: bool) -> String {
    let strip = strip(summary, agent);
    let mut text = format!(
        "✦ hollywood  ■ {} studio{}",
        strip.studios,
        if strip.studios == 1 { "" } else { "s" }
    );
    if strip.waiting > 0 {
        text.push_str(&format!("  !{} ", strip.waiting));
    }
    text
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

/// One cast diamond's colour class: the role's state, as the frame's
/// `◆` shows it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum CastMark {
    /// Working — green.
    Working,
    /// Something waits on a person — peach.
    Attention,
    /// Failed or lost — red.
    Failed,
    /// Anything else — dim.
    Idle,
}

impl CastMark {
    pub fn of(role: &RoleSummary) -> CastMark {
        if role.attention.is_some() {
            CastMark::Attention
        } else if role.failed.is_some() || role.state == "lost" {
            CastMark::Failed
        } else if role.state == "live" && role.work == Some(Work::Working) {
            CastMark::Working
        } else {
            CastMark::Idle
        }
    }
}
