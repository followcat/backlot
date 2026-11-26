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
    /// A needs-you attention or an unanswered question on (studio,
    /// role) — Enter focuses the role's terminal, `p` peeks.
    Role(StudioKey, String),
    /// A studio with no requirement — Enter focuses its producer, so
    /// the person can tell it what to build.
    Tell(StudioKey),
    /// A studio whose ledger holds unknown event kinds from a newer backlot.
    Upgrade(StudioKey),
    /// A studio waiting for user opening-design review, reply, or sign-off.
    Decision(StudioKey),
    /// A machine whose stage is down (`offline == false`) or that is
    /// unreachable — `r` restarts the stage.
    Stage(Option<String>, bool),
    /// An open card row — `a` applies, `x` cancels, `s`/Enter shows.
    Card(String),
    /// An open outward push/PR card row with studio/machine ownership.
    OutwardCard(StudioKey, String),
    /// A ready-to-finish studio — `f` finishes, Enter focuses it.
    Ready(StudioKey),
    /// The waiting row for Hollywood's own prompt / block — `y`/`d`
    /// answer a prompt, `p` peeks, `open ↵` focuses `bl-hollywood`.
    Hollywood,
    /// The live row under the newest `user ›` line — `p` peeks,
    /// Enter focuses `bl-hollywood`. Kept distinct from `Hollywood`
    /// so `Wait::Hollywood` and `Row::Live` never share selection.
    HollywoodLive,
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

/// The live line under the newest `user ›` line: what Hollywood is
/// doing with it — the same activity a role's row carries.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Live<'a> {
    /// The line still sits in the outbox behind the send gate.
    Pending,
    /// Queued while a make-before-break handoff is running.
    HandoffQueued {
        runner: &'a str,
        model: &'a str,
        effort: Option<&'a str>,
    },
    /// Queued while current Hollywood is blocked (e.g. out of quota).
    BlockedQueue(&'a str),
    /// Hollywood is working; its terminal's `doing`, when reported.
    Working(Option<&'a str>),
    /// A user-owned permission prompt stands on its screen.
    Prompt(&'a str),
    /// Hollywood's screen reads idle and the line still has no answer
    /// — a reply that never came. The row offers `open ↵`.
    Idle,
}

/// One band row, borrowing its data from the documents.
pub(crate) enum Row<'a> {
    /// The newest `hollywood.reported` headline.
    Headline(&'a str),
    /// A waiting item — an index into the sheet's `waits`.
    Wait(usize),
    /// `✓ nothing waiting on you across <S> studios` — the calm row,
    /// shown only while the needs-you list is empty.
    Calm { studios: usize },
    /// A line of the user's exchange with Hollywood — the dialog's
    /// half drawn above the input (D21).
    Talk(&'a Exchange),
    /// `… <n> earlier · ↵ read` — the lines older than the last two
    /// turns, folded into one dim row. `newest` is the last folded
    /// entry, the one Enter opens.
    Fold { count: usize, newest: u64 },
    /// Hollywood's live progress under the newest unanswered `user ›`
    /// line — pending, working, idle, or a user-owned prompt.
    Live(Live<'a>),
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
            Row::Fold { newest, .. } => Target::Talk(*newest),
            Row::Live(_) => Target::HollywoodLive,
            Row::Wait(i) => waits[*i].target(),
            Row::Calm { .. } => Target::Calm,
            Row::Input => Target::Input,
            Row::Setup => Target::Setup,
        }
    }
}

/// The studio a waiting row belongs to, for the Call Sheet's
/// one-row-per-studio projection. Hollywood cards are local rows.
pub(crate) fn wait_studio_key<'a>(wait: &Wait<'a>) -> Option<(Option<&'a str>, &'a str)> {
    match wait {
        Wait::Prompt(item) => Some((item.machine, item.studio.id.as_str())),
        Wait::Role(item) => Some((item.machine, item.studio.id.as_str())),
        Wait::Upgrade(item) => Some((item.machine, item.studio.id.as_str())),
        Wait::Decision(item) => Some((item.machine, item.studio.id.as_str())),
        Wait::NoRequirement(item) => Some((item.machine, item.studio.id.as_str())),
        Wait::Outward(item) => Some((item.machine, item.studio.id.as_str())),
        Wait::Ready(item) => Some((item.machine, item.studio.id.as_str())),
        Wait::Card(card) => Some((None, card.studio.as_str())),
        Wait::Hollywood(_) | Wait::Stage(_) => None,
    }
}

/// How many rows folded into this row's `+n`: the studio's other
/// waiting decisions. Reports never enter this list at all.
pub(crate) fn group_extra(waits: &[Wait<'_>], wait: &Wait<'_>) -> usize {
    let Some(key) = wait_studio_key(wait) else {
        return 0;
    };
    waits
        .iter()
        .filter(|w| wait_studio_key(w) == Some(key))
        .count()
        .saturating_sub(1)
}

/// One row index per studio: the first (most urgent) item leads, except
/// that READY always leads its studio and absorbs that studio's asks.
fn band_wait_indexes(waits: &[Wait<'_>]) -> Vec<usize> {
    let mut indexes = Vec::new();
    let mut slots = std::collections::BTreeMap::new();
    for (i, wait) in waits.iter().enumerate() {
        let Some((machine, studio)) = wait_studio_key(wait) else {
            indexes.push(i);
            continue;
        };
        let key = (machine.map(str::to_string), studio.to_string());
        match slots.get(&key).copied() {
            None => {
                slots.insert(key, indexes.len());
                indexes.push(i);
            }
            Some(slot) => {
                let lead = &waits[indexes[slot]];
                if matches!(wait, Wait::Ready(_)) && !matches!(lead, Wait::Ready(_)) {
                    indexes[slot] = i;
                }
            }
        }
    }
    indexes
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
        && let Some(hw) = summary.and_then(|s| s.hollywood.as_ref())
        && let Some(headline) = hw.headline.as_deref()
    {
        // R3-M1: the newest exchange line already prints this
        // headline — drawing both doubles the report in one band.
        let newest_reply = hw.exchange.iter().rev().find(|e| e.who == "hollywood");
        let duplicated = newest_reply
            .is_some_and(|e| hw.headline_seq == Some(e.seq) || e.text.trim() == headline.trim());
        if !duplicated {
            out.push(Row::Headline(headline));
        }
    }
    let waits = waiting_items(machines, status);
    if waits.is_empty() {
        out.push(Row::Calm {
            studios: crate::panel::data::studio_count(machines),
        });
    } else {
        out.extend(band_wait_indexes(&waits).into_iter().map(Row::Wait));
    }
    // The dialog's visible half: the newest turn and the turn before
    // it, oldest first, right above the input they continue into.
    // Older lines fold into one dim row. Hollywood's live row follows
    // the newest `user ›` line while that line has no reply after it.
    if agent {
        let talk = summary
            .and_then(|s| s.hollywood.as_ref())
            .map(|h| h.exchange.as_slice())
            .unwrap_or(&[]);
        let start = visible_start(talk);
        if start > 0 {
            out.push(Row::Fold {
                count: start,
                newest: talk[start - 1].seq,
            });
        }
        let live = hollywood_live(machines);
        for (i, line) in talk.iter().enumerate().skip(start) {
            out.push(Row::Talk(line));
            if let Some((index, live)) = live
                && index == i
            {
                out.push(Row::Live(live));
            }
        }
    }
    out.push(if agent { Row::Input } else { Row::Setup });
    (waits, out)
}

/// The index the talk window starts at: a turn is a `user ›` line and
/// everything after it up to the next one, and the newest turn plus
/// the turn before it stay; with no user line the newest two lines.
pub(crate) fn visible_start(talk: &[Exchange]) -> usize {
    let users: Vec<usize> = talk
        .iter()
        .enumerate()
        .filter(|(_, e)| e.who == "user")
        .map(|(i, _)| i)
        .collect();
    match users.len() {
        0 => talk.len().saturating_sub(2),
        n => users[n.saturating_sub(2)],
    }
}

/// The live row and the exchange index it follows: the newest `user ›`
/// line with no Hollywood reply after it — a report or a card (both
/// `who: hollywood`). A prompt wins over a pending line (it is what
/// blocks the delivery the person must clear), which wins over
/// `working`, which wins over an idle screen that still has not
/// answered; a calm Hollywood with no unanswered line shows nothing.
pub(crate) fn hollywood_live(machines: &[MachineSummary]) -> Option<(usize, Live<'_>)> {
    let hw = local_summary(machines)?.hollywood.as_ref()?;
    let newest_user = hw.exchange.iter().rposition(|e| e.who == "user")?;
    if hw.exchange[newest_user + 1..]
        .iter()
        .any(|e| e.who == "hollywood")
    {
        return None;
    }
    let live = if let Some(hf) = &hw.handoff
        && super::handoff_ui::is_handoff_active(hw)
        && (hw.pending || hw.queue.is_some())
    {
        Live::HandoffQueued {
            runner: hf.target_runner.as_str(),
            model: hf.target_model.as_str(),
            effort: hf.target_effort.as_deref(),
        }
    } else if super::handoff_ui::is_quota_or_error(hw) && (hw.pending || hw.queue.is_some()) {
        Live::BlockedQueue(
            hw.attention_detail
                .as_deref()
                .or(hw.attention.as_deref())
                .unwrap_or("blocked"),
        )
    } else if hw.attention_user {
        Live::Prompt(
            hw.attention_detail
                .as_deref()
                .or(hw.attention.as_deref())
                .unwrap_or("permission prompt"),
        )
    } else if hw.pending {
        Live::Pending
    } else if hw.work == Some(Work::Working) {
        Live::Working(hw.doing.as_deref())
    } else if hw.work == Some(Work::Waiting) {
        Live::Idle
    } else {
        return None;
    };
    Some((newest_user, live))
}

/// Whether Hollywood owns a prompt its `y`/`d` can answer — the one
/// thing the live row's keys and the footer name. A prompt on a runner
/// with no approval key (`can_answer_prompt` false) stays on the row
/// but is answered in its terminal, so this is false for it.
pub(crate) fn hollywood_asking(machines: &[MachineSummary]) -> bool {
    local_summary(machines)
        .and_then(|s| s.hollywood.as_ref())
        .is_some_and(|h| h.attention_user && h.can_answer_prompt)
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

/// Hollywood's live liveness / outbox state for the Call Sheet and Lot Pill.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub(crate) enum HollywoodActivity {
    #[default]
    Idle,
    /// A `tell hollywood` command is in flight or an undelivered
    /// message sits in Hollywood's outbox behind the send gate.
    Queued,
    /// Hollywood's terminal is actively `Working`.
    Working,
}

impl HollywoodActivity {
    pub(crate) fn is_active(self) -> bool {
        !matches!(self, HollywoodActivity::Idle)
    }
}

/// Derive Hollywood's activity from the local summary and in-flight
/// `ActionTracker` marks.
pub(crate) fn hollywood_activity(
    machines: &[MachineSummary],
    pending_marks: Option<&str>,
) -> HollywoodActivity {
    let hw = local_summary(machines).and_then(|s| s.hollywood.as_ref());
    if hw.is_some_and(super::handoff_ui::is_handoff_active) {
        return HollywoodActivity::Queued;
    }
    if hw.is_some_and(|h| h.state == "live" && h.work == Some(Work::Working)) {
        return HollywoodActivity::Working;
    }
    let in_flight = pending_marks.is_some_and(|p| p.split_whitespace().any(|m| m == "hollywood"));
    if in_flight || hw.is_some_and(|h| h.state != "down" && h.pending) {
        return HollywoodActivity::Queued;
    }
    HollywoodActivity::Idle
}

/// Highest `seq` among Hollywood's own replies (`who == "hollywood"`
/// in `exchange` plus `headline_seq`), or `0` when none exist.
pub(crate) fn hollywood_reply_max_seq(
    hw: Option<&backlot_model::summary::HollywoodSummary>,
) -> u64 {
    let Some(hw) = hw else { return 0 };
    hw.exchange
        .iter()
        .filter(|e| e.who == "hollywood")
        .map(|e| e.seq)
        .chain(hw.headline_seq)
        .max()
        .unwrap_or(0)
}

/// Count of new Hollywood replies (`who == "hollywood"` in `exchange`,
/// plus `headline_seq` when absent from `exchange`) with `seq > seen`
/// — strictly excluding the user's own `tell` entries.
pub(crate) fn hollywood_unread_count(
    hw: Option<&backlot_model::summary::HollywoodSummary>,
    seen: u64,
) -> usize {
    let Some(hw) = hw else { return 0 };
    let in_exchange = hw
        .exchange
        .iter()
        .filter(|e| e.who == "hollywood" && e.seq > seen)
        .count();
    let extra_headline = hw.headline_seq.is_some_and(|hseq| {
        hseq > seen
            && !hw
                .exchange
                .iter()
                .any(|e| e.who == "hollywood" && e.seq == hseq)
    });
    in_exchange + usize::from(extra_headline)
}
