//! Studio events, derived from the `backlot` documents alone (W2, N1).
//!
//! The client already knows what waits on the person — `waiting_items`
//! builds that one list for the Call Sheet, the lot pill and the calm
//! row. This module turns the same list into notifications: each item
//! the studio layer can see becomes at most one event, keyed so a
//! republish of the same state never fires twice and the first document
//! after start seeds silently.
//!
//! Pure: no `App`, no clocks, no I/O. The wiring lives beside it in
//! `mod.rs`.

use crate::hollywood::{Conn, Wait, waiting_items};
use crate::panel::data::MachineSummary;

/// A machine's line and whether its stage answers — the closure
/// `hollywood::status_of` builds from the view.
pub(crate) type Status<'a> = dyn Fn(Option<&str>) -> (Conn, bool) + 'a;

/// What a studio event is about — the sort the toast titles and sounds
/// follow.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum StudioEventKind {
    /// A role's newest unanswered `tell user` — `producer asks: …`.
    Asks,
    /// A needs-you attention that is not a prompt.
    RoleNeedsYou,
    /// A user-owned permission prompt on a role's screen.
    RolePrompt,
    /// A user-owned permission prompt on `bl-hollywood`'s screen.
    HollywoodNeedsYou,
    /// A new `hollywood.reported` headline in the local exchange.
    HollywoodReply,
    /// An open card waiting for the person.
    Card,
    /// Every task terminal, `studio.finished` still owed.
    Ready,
    /// A machine whose stage stopped while it has live studios.
    StageStopped,
}

/// One studio event: its dedup key, its toast text and the needs-you
/// target its toast jumps to.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct StudioEvent {
    pub kind: StudioEventKind,
    /// Stable identity across publishes — one event per ledger seq, card
    /// or studio state.
    pub key: String,
    pub title: String,
    pub body: Option<String>,
    pub target: crate::hollywood::Target,
    /// The desktop ping text for the two event kinds the person asked to
    /// be told about — a role's `--ask` and a card waiting — and `None`
    /// for every other kind.
    pub ping: Option<String>,
}

/// ` · host-b` for a remote studio, nothing on this core — the same
/// rule the dock's labels follow, so a lone machine keeps short texts.
fn suffix(machine: Option<&str>) -> String {
    machine.map(|m| format!(" · {m}")).unwrap_or_default()
}

/// The item's one-line explanation: the prompt's detail, else its
/// reason word.
fn prompt_text<'a>(detail: Option<&'a str>, reason: Option<&'a str>) -> &'a str {
    detail.or(reason).unwrap_or("permission prompt")
}

/// The ledger seq of a role's newest user-facing exchange line — the
/// `tell user` behind its `asks` — so the key is the ledger's own
/// identity, not the question's wording.
fn ask_seq(studio: &backlot_model::summary::StudioSummary, role: &str) -> Option<u64> {
    let text = studio
        .roles
        .iter()
        .find(|r| r.role == role)?
        .asks
        .as_deref()?;
    studio
        .exchange
        .iter()
        .rev()
        .find(|line| line.who == role && line.text.starts_with(text))
        .map(|line| line.seq)
        .or_else(|| {
            // The exchange window moved past the question; its own line
            // still carries the seq when it is in the window at all.
            studio
                .exchange
                .iter()
                .rev()
                .find(|line| line.who == role)
                .map(|line| line.seq)
        })
}

/// The newest Hollywood reply in the local exchange, with its seq.
fn newest_reply(machines: &[MachineSummary]) -> Option<(u64, String)> {
    let hollywood = crate::hollywood::local_summary(machines)?
        .hollywood
        .as_ref()?;
    hollywood
        .exchange
        .iter()
        .rev()
        .find(|line| line.who == "hollywood")
        .map(|line| (line.seq, line.text.clone()))
        .or_else(|| {
            let seq = hollywood.headline_seq?;
            hollywood.headline.as_ref().map(|text| (seq, text.clone()))
        })
}

/// The desktop ping's length budget — the outer terminal truncates its
/// own way, but a one-line body reads best short.
const PING_MAX: usize = 120;

/// The first line of a multi-line question, trimmed — what the ping
/// shows after `asks:`.
fn first_line(text: &str) -> &str {
    text.lines().next().unwrap_or("").trim()
}

/// A desktop ping's text, truncated to one short line.
fn ping_text(text: String) -> String {
    crate::chrome::text::truncate_end(&text, PING_MAX)
}

/// Every studio event the documents describe right now, in the
/// needs-you list's own order. A blocked Hollywood and an unreachable
/// machine are not events: N1 names prompts, asks, replies, cards,
/// ready studios and stopped stages only.
pub(crate) fn derive(machines: &[MachineSummary], status: &Status<'_>) -> Vec<StudioEvent> {
    let mut out = Vec::new();
    for wait in waiting_items(machines, status) {
        let target = wait.target();
        let event = match &wait {
            Wait::Prompt(item) => StudioEvent {
                kind: StudioEventKind::RolePrompt,
                key: format!(
                    "rprompt\0{}\0{}\0{}\0{}",
                    item.machine.unwrap_or(""),
                    item.studio.id,
                    item.role.role,
                    item.role.attention_seq.unwrap_or(0)
                ),
                title: format!(
                    "{} needs you: {}{}",
                    item.role.name,
                    prompt_text(
                        item.role.attention_detail.as_deref(),
                        item.role.attention.as_deref()
                    ),
                    suffix(item.machine)
                ),
                body: None,
                target,
                ping: None,
            },
            Wait::Role(item) if item.asks => {
                let seq = ask_seq(item.studio, &item.role.role);
                let question = item.role.asks.as_deref().unwrap_or("a question");
                StudioEvent {
                    kind: StudioEventKind::Asks,
                    key: format!(
                        "asks\0{}\0{}\0{}\0{}",
                        item.machine.unwrap_or(""),
                        item.studio.id,
                        item.role.role,
                        seq.map(|s| s.to_string()).unwrap_or_else(|| item
                            .role
                            .asks
                            .clone()
                            .unwrap_or_default())
                    ),
                    title: format!(
                        "{} asks: {}{}",
                        item.role.name,
                        question,
                        suffix(item.machine)
                    ),
                    body: None,
                    target,
                    // The person asked to be told: `<studio> · <role> asks: <first line>`.
                    ping: Some(ping_text(format!(
                        "{} · {} asks: {}",
                        item.studio.id,
                        item.role.name,
                        first_line(question)
                    ))),
                }
            }
            Wait::Role(item) => StudioEvent {
                kind: StudioEventKind::RoleNeedsYou,
                key: format!(
                    "rneeds\0{}\0{}\0{}\0{}",
                    item.machine.unwrap_or(""),
                    item.studio.id,
                    item.role.role,
                    item.role.attention_seq.unwrap_or(0)
                ),
                title: format!(
                    "{} needs you: {}{}",
                    item.role.name,
                    prompt_text(
                        item.role.attention_detail.as_deref(),
                        item.role.attention.as_deref()
                    ),
                    suffix(item.machine)
                ),
                body: None,
                target,
                ping: None,
            },
            Wait::Hollywood(hw) if hw.attention_user => StudioEvent {
                kind: StudioEventKind::HollywoodNeedsYou,
                key: format!(
                    "hneeds\0{}",
                    prompt_text(hw.attention_detail.as_deref(), hw.attention.as_deref())
                ),
                title: format!(
                    "hollywood needs you: {}",
                    prompt_text(hw.attention_detail.as_deref(), hw.attention.as_deref())
                ),
                body: None,
                target,
                ping: None,
            },
            Wait::Card(card) => StudioEvent {
                kind: StudioEventKind::Card,
                key: format!("card\0{}", card.id),
                title: format!("card waiting: {}", card.action),
                body: Some(format!("{} · {}", card.studio, card.why)),
                target,
                // A card waiting for the person: `<studio> · card c<n>: <summary>`.
                ping: Some(ping_text(format!(
                    "{} · card {}: {}",
                    card.studio, card.id, card.action
                ))),
            },
            Wait::Outward(item) => StudioEvent {
                kind: StudioEventKind::Card,
                key: format!(
                    "outward\0{}\0{}\0{}",
                    item.machine.unwrap_or(""),
                    item.studio.id,
                    item.card.id
                ),
                title: format!("card waiting: {}", item.card.command),
                body: Some(format!("{} · {}", item.studio.id, item.card.why)),
                target,
                ping: Some(ping_text(format!(
                    "{} · card {}: {}",
                    item.studio.id, item.card.id, item.card.command
                ))),
            },
            Wait::Ready(item) => StudioEvent {
                kind: StudioEventKind::Ready,
                key: format!("ready\0{}\0{}", item.machine.unwrap_or(""), item.studio.id),
                title: format!(
                    "{} is ready to finish{}",
                    item.studio.id,
                    suffix(item.machine)
                ),
                body: Some(format!(
                    "{}/{} tasks accepted",
                    item.studio.tasks_done, item.studio.tasks_total
                )),
                target,
                ping: None,
            },
            Wait::Stage(item) if !item.offline => {
                let machine = item.machine.unwrap_or(crate::project::LOCAL_LABEL);
                StudioEvent {
                    kind: StudioEventKind::StageStopped,
                    key: format!("stage\0{}", item.machine.unwrap_or("")),
                    title: format!("stage stopped on {machine}"),
                    body: Some(format!(
                        "{} studio{} waiting on it",
                        item.studios,
                        if item.studios == 1 { "" } else { "s" }
                    )),
                    target,
                    ping: None,
                }
            }
            // A blocked Hollywood with no prompt, an unreachable machine,
            // and a studio with no requirement are not N1's events.
            _ => continue,
        };
        out.push(event);
    }
    if let Some((seq, text)) = newest_reply(machines) {
        out.push(StudioEvent {
            kind: StudioEventKind::HollywoodReply,
            key: format!("hreply\0{seq}"),
            title: format!("hollywood replied: {text}"),
            body: None,
            target: crate::hollywood::Target::Talk(seq),
            ping: None,
        });
    }
    out
}

#[cfg(test)]
#[path = "events_tests.rs"]
mod tests;
