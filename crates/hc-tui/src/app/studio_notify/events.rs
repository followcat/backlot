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
            },
            Wait::Role(item) if item.asks => {
                let seq = ask_seq(item.studio, &item.role.role);
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
                        item.role.asks.as_deref().unwrap_or("a question"),
                        suffix(item.machine)
                    ),
                    body: None,
                    target,
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
            },
            Wait::Card(card) => StudioEvent {
                kind: StudioEventKind::Card,
                key: format!("card\0{}", card.id),
                title: format!("card waiting: {}", card.action),
                body: Some(format!("{} · {}", card.studio, card.why)),
                target,
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
        });
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::hollywood::Conn;

    /// The dedup key's own join — tests spell keys part by part so no
    /// literal ever needs a `\0` escape.
    fn key(parts: &[&str]) -> String {
        parts.join("\0")
    }

    fn up(_: Option<&str>) -> (Conn, bool) {
        (Conn::Online, true)
    }

    /// Stage up, or a stopped stage — a plain fn item, so the
    /// higher-ranked `Status` bound accepts it.
    fn stopped_stage(_: Option<&str>) -> (Conn, bool) {
        (Conn::Online, false)
    }

    fn unreachable(_: Option<&str>) -> (Conn, bool) {
        (Conn::Offline, false)
    }

    fn summary(value: serde_json::Value) -> MachineSummary {
        MachineSummary {
            machine: None,
            summary: serde_json::from_value(value).unwrap(),
        }
    }

    /// A document with one `acme-shop` studio: `roles` and `studio` go
    /// on the studio, `doc` on the document itself (Hollywood lives at
    /// the root, not on a studio).
    fn doc_json(
        roles: serde_json::Value,
        studio: serde_json::Value,
        doc: serde_json::Value,
    ) -> serde_json::Value {
        let mut entry = serde_json::json!({
            "id": "acme-shop", "title": "Acme shop", "state": "live",
            "requirement_version": 1, "mode": "standard",
            "open_reviews": 0, "waiting": 0, "roles": roles,
        });
        for (key, value) in studio.as_object().unwrap() {
            entry[key] = value.clone();
        }
        let mut out = serde_json::json!({
            "schema": "backlot/1", "updated_ms": 1, "studios": [entry],
        });
        for (key, value) in doc.as_object().unwrap() {
            out[key] = value.clone();
        }
        out
    }

    fn studio_json(roles: serde_json::Value, studio: serde_json::Value) -> serde_json::Value {
        doc_json(roles, studio, serde_json::json!({}))
    }

    #[test]
    fn an_ask_keys_on_its_ledger_seq_and_names_the_question() {
        let machines = [summary(studio_json(
            serde_json::json!([{ "role": "producer", "name": "producer",
                "state": "live", "asks": "which shop?" }]),
            serde_json::json!({ "exchange": [
                { "seq": 4, "who": "producer", "text": "which shop?" }
            ]}),
        ))];
        let events = derive(&machines, &up);
        let asks = events
            .iter()
            .find(|e| e.kind == StudioEventKind::Asks)
            .expect("the ask");
        assert_eq!(asks.title, "producer asks: which shop?");
        assert_eq!(asks.key, key(&["asks", "", "acme-shop", "producer", "4"]));
    }

    #[test]
    fn a_user_prompt_and_a_needs_you_attention_read_differently() {
        let machines = [summary(studio_json(
            serde_json::json!([
                { "role": "producer", "name": "producer", "state": "live",
                  "attention": "permission prompt — needs the user",
                  "attention_user": true, "attention_seq": 7,
                  "attention_detail": "run npm test" },
                { "role": "writer", "name": "writer", "state": "live",
                  "attention": "over budget", "attention_class": "needs_you",
                  "attention_seq": 9, "attention_detail": "budget exhausted" }
            ]),
            serde_json::json!({}),
        ))];
        let events = derive(&machines, &up);
        let prompt = events
            .iter()
            .find(|e| e.kind == StudioEventKind::RolePrompt)
            .expect("the prompt");
        assert_eq!(prompt.title, "producer needs you: run npm test");
        assert_eq!(
            prompt.key,
            key(&["rprompt", "", "acme-shop", "producer", "7"])
        );
        assert!(matches!(
            prompt.target,
            crate::hollywood::Target::Prompt(..)
        ));
        let needs = events
            .iter()
            .find(|e| e.kind == StudioEventKind::RoleNeedsYou)
            .expect("the needs-you attention");
        assert_eq!(needs.title, "writer needs you: budget exhausted");
        assert!(matches!(needs.target, crate::hollywood::Target::Role(..)));
    }

    #[test]
    fn a_reply_a_card_a_ready_studio_and_a_stopped_stage_each_fire() {
        let machines = [summary(doc_json(
            serde_json::json!([{ "role": "producer", "name": "producer", "state": "closed" }]),
            serde_json::json!({
                "ready_to_finish": true, "tasks_done": 2, "tasks_total": 2,
            }),
            serde_json::json!({
                "hollywood": {
                    "state": "live",
                    "headline": "all quiet", "headline_seq": 12,
                    "exchange": [{ "seq": 12, "who": "hollywood", "text": "all quiet" }],
                    "cards": [{ "id": "c3", "studio": "acme-shop",
                                "action": "pause t1", "why": "blocked" }]
                }
            }),
        ))];
        let events = derive(&machines, &stopped_stage);
        let by = |kind: StudioEventKind| {
            events
                .iter()
                .find(|e| e.kind == kind)
                .unwrap_or_else(|| panic!("{kind:?}"))
        };
        assert_eq!(
            by(StudioEventKind::HollywoodReply).key,
            key(&["hreply", "12"])
        );
        assert_eq!(by(StudioEventKind::Card).key, key(&["card", "c3"]));
        assert_eq!(by(StudioEventKind::Card).title, "card waiting: pause t1");
        assert_eq!(
            by(StudioEventKind::Ready).title,
            "acme-shop is ready to finish"
        );
        assert_eq!(
            by(StudioEventKind::Ready).key,
            key(&["ready", "", "acme-shop"])
        );
        assert_eq!(
            by(StudioEventKind::StageStopped).title,
            "stage stopped on Local"
        );
    }

    #[test]
    fn a_blocked_hollywood_an_unreachable_machine_and_no_requirement_stay_quiet() {
        let machines = [summary(serde_json::json!({
            "schema": "backlot/1", "updated_ms": 1,
            "studios": [{
                "id": "acme-shop", "title": "Acme shop", "state": "live",
                "requirement_version": 0, "mode": "standard",
                "open_reviews": 0, "waiting": 0,
                "roles": [{ "role": "producer", "name": "producer", "state": "live" }]
            }],
            "hollywood": { "state": "live", "work": "blocked" }
        }))];
        assert!(derive(&machines, &unreachable).is_empty());
    }
}
