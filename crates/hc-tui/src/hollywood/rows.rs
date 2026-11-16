//! The layer's row model: a pure `(Summary, stage_running)` → rows
//! map for the expanded panel, plus the collapsed strip's one-line
//! content. Every row is a single line so scroll math stays in
//! one-line steps. All data is the `backlot` document's — nothing is
//! scraped from Hollywood's terminal.

use backlot_model::summary::{StudioSummary, Summary};

/// A selected/clicked row's stable identity. Rows rebuild per frame,
/// so anything the user can act on is named by document ids, never by
/// position.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Target {
    /// The `✦ hollywood …` title row — Enter focuses `bl-hollywood`.
    Title,
    /// The "stage not running" line — shown, not actionable.
    StageDown,
    /// The "no studios" line — shown, not actionable.
    Empty,
    /// A studio row — Enter focuses that studio's producer through
    /// the studio panel's own path.
    Studio(String),
    /// Hollywood's newest headline — shown, not actionable.
    Headline,
    /// An open card row — `a` applies, `c` cancels, `s`/Enter shows.
    Card(String),
    /// The input line — `i`/Enter opens it for editing.
    Input,
}

/// One display row, borrowing its data from the summary.
#[derive(Debug)]
pub(crate) enum Row<'a> {
    /// `✦ hollywood <state> <runner>/<model>/<effort>`
    Title,
    /// `stage not running` — same one-line read as the panel's.
    StageDown,
    /// `no studios` — a missing, malformed or foreign-schema document.
    Empty,
    /// `<id> · <state> · !<waiting> · <what it is doing>`.
    Studio(&'a StudioSummary),
    /// The newest `hollywood.reported` headline.
    Headline(&'a str),
    /// `c<n> · <studio> · <action> — <why>` with its Apply/Cancel/Show
    /// targets.
    Card(&'a backlot_model::summary::CardSummary),
    /// `› <input>` — the `tell hollywood` line.
    Input,
}

impl Row<'_> {
    /// The row's stable identity for selection, clicks and Enter.
    pub fn target(&self) -> Target {
        match self {
            Row::Title => Target::Title,
            Row::StageDown => Target::StageDown,
            Row::Empty => Target::Empty,
            Row::Studio(studio) => Target::Studio(studio.id.clone()),
            Row::Headline(_) => Target::Headline,
            Row::Card(card) => Target::Card(card.id.clone()),
            Row::Input => Target::Input,
        }
    }
}

/// The expanded panel's rows: the fixed title, then the scrolled
/// body — a stage-down line, the studios, the newest headline, the
/// open cards — then the fixed input line. The studio rows read the
/// document even while Hollywood's own agent is down.
pub(crate) fn rows<'a>(summary: Option<&'a Summary>, stage_running: bool) -> Vec<Row<'a>> {
    let mut out = vec![Row::Title];
    if !stage_running {
        out.push(Row::StageDown);
    }
    match summary {
        Some(summary) => {
            if summary.studios.is_empty() {
                out.push(Row::Empty);
            } else {
                out.extend(summary.studios.iter().map(Row::Studio));
            }
            if let Some(hollywood) = summary.hollywood.as_ref()
                && let Some(headline) = hollywood.headline.as_deref()
            {
                out.push(Row::Headline(headline));
            }
            if let Some(hollywood) = summary.hollywood.as_ref() {
                out.extend(hollywood.cards.iter().map(Row::Card));
            }
        }
        None => out.push(Row::Empty),
    }
    out.push(Row::Input);
    out
}

/// The collapsed strip's one-line content (D16): Hollywood's mark
/// and role state, the count of studios by state, the count of open
/// cards, then the newest headline. `draw` cuts it to a third of the
/// frame's width.
pub(crate) fn strip_text(summary: Option<&Summary>) -> String {
    let mut text = "✦ hollywood".to_string();
    let (studios, hollywood) = match summary {
        Some(summary) => (summary.studios.as_slice(), summary.hollywood.as_ref()),
        None => (&[][..], None),
    };
    if let Some(hollywood) = hollywood {
        text.push_str(&format!(" {}", hollywood.state));
    } else {
        text.push_str(" down");
    }
    let live = studios.iter().filter(|s| s.state == "live").count();
    let finished = studios.len() - live;
    match (live, finished) {
        (0, 0) => text.push_str(" · 0 studios"),
        (l, 0) => text.push_str(&format!(" · {l} live")),
        (0, f) => text.push_str(&format!(" · {f} finished")),
        (l, f) => text.push_str(&format!(" · {l} live {f} fin")),
    }
    let cards = hollywood.map(|h| h.cards.len()).unwrap_or(0);
    if cards > 0 {
        text.push_str(&format!(
            " · {cards} card{}",
            if cards == 1 { "" } else { "s" }
        ));
    }
    if let Some(headline) = hollywood.and_then(|h| h.headline.as_deref()) {
        text.push_str(&format!(" · {headline}"));
    }
    text
}
