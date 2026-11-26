//! Pure summary resolvers mapping `StudioSummary`, `HollywoodSummary`, and
//! `CardSummary` rows into structured [`ReaderMeta`].

use backlot_model::summary::HollywoodSummary;

use crate::hollywood::{
    self, ReaderKind, ReaderMeta, ReaderOrigin, find_studio_role, hollywood_state_word,
    role_state_word,
};
use crate::panel::data::{self, MachineSummary};
use crate::studio::StudioKey;
use crate::studio::glyphs::Person;

pub(super) fn hollywood_summary(machines: &[MachineSummary]) -> Option<&HollywoodSummary> {
    hollywood::local_summary(machines).and_then(|s| s.hollywood.as_ref())
}

pub(super) fn hollywood_base_meta(
    hw: Option<&HollywoodSummary>,
    hw_terminal: Option<String>,
) -> ReaderMeta {
    let Some(h) = hw else {
        return ReaderMeta::default();
    };
    ReaderMeta {
        role: Some("hollywood".into()),
        role_state: Some(hollywood_state_word(h).into()),
        role_terminal: hw_terminal.filter(|_| h.state == "live"),
        attention_user: h.attention_user,
        runner: h.runner.clone(),
        model: h.model.clone(),
        ..ReaderMeta::default()
    }
}

pub(super) fn hollywood_exchange_meta(
    hw: Option<&HollywoodSummary>,
    hw_terminal: Option<String>,
    seq: u64,
) -> ReaderMeta {
    let mut meta = hollywood_base_meta(hw, hw_terminal);
    meta.studio = Some("hollywood".into());
    meta.seq = Some(seq);
    meta.origin = ReaderOrigin::HollywoodExchange(seq);
    meta.kind = ReaderKind::Report;

    let Some(h) = hw else {
        return meta;
    };
    if let Some((idx, entry)) = h.exchange.iter().enumerate().find(|(_, e)| e.seq == seq) {
        if entry.who == "user" {
            meta.kind = ReaderKind::Tell;
        } else if entry.kind.as_deref() == Some("card") {
            // A card line reads as the card it names: its open summary
            // carries the studio, the action and the open status; a card
            // no longer open carries its own state from the ledger.
            meta.kind = ReaderKind::Card;
            let id = format!("c{seq}");
            if let Some(card) = h.cards.iter().find(|c| c.id == id) {
                meta.studio = Some(card.studio.clone());
                meta.card_action = Some(card.action.clone());
                meta.card_status = Some("open".into());
                meta.ts_ms = card.ts_ms;
            } else {
                meta.card_status = entry.card_state.clone();
            }
        } else {
            meta.kind = ReaderKind::Report;
        }
        meta.from = Some(entry.who.clone());
        meta.to = entry.to.clone();
        meta.ts_ms = meta.ts_ms.or(entry.ts_ms);
        meta.nav_index = Some(idx + 1);
        meta.nav_total = Some(h.exchange.len());
    } else {
        meta.from = Some("hollywood".into());
    }
    meta
}

pub(super) fn card_meta(machines: &[MachineSummary], id: &str) -> ReaderMeta {
    let hw = hollywood_summary(machines);
    let mut meta = ReaderMeta {
        kind: ReaderKind::Card,
        from: Some("hollywood".into()),
        card_status: Some("open".into()),
        origin: ReaderOrigin::Card(id.to_string()),
        ..ReaderMeta::default()
    };
    let Some(h) = hw else {
        return meta;
    };
    if let Some((idx, card)) = h.cards.iter().enumerate().find(|(_, c)| c.id == id) {
        meta.studio = Some(card.studio.clone());
        meta.machine = data::studios(machines)
            .into_iter()
            .find(|s| s.studio.id == card.studio)
            .and_then(|s| s.machine.map(str::to_owned));
        meta.ts_ms = card.ts_ms;
        meta.card_action = Some(card.action.clone());
        meta.nav_index = Some(idx + 1);
        meta.nav_total = Some(h.cards.len());
    } else if let Some(seq) = id.strip_prefix('c').and_then(|s| s.parse::<u64>().ok())
        && let Some(line) = h
            .exchange
            .iter()
            .find(|e| e.seq == seq && e.kind.as_deref() == Some("card"))
    {
        // A card no longer open still reads: its state comes from the
        // ledger line, so a withdrawn card shows as withdrawn.
        meta.card_status = line.card_state.clone();
        meta.ts_ms = line.ts_ms;
    }
    meta
}

pub(super) fn records_meta(machines: &[MachineSummary], key: StudioKey) -> ReaderMeta {
    let studio = data::studio(machines, &key);
    ReaderMeta {
        kind: ReaderKind::Records,
        studio: Some(key.id.clone()),
        machine: key.machine.clone(),
        studio_counts: studio.map(|s| {
            (
                s.studio.tasks_done,
                s.studio.tasks_total,
                s.studio.roles.len(),
            )
        }),
        studio_finished: studio.is_some_and(|s| s.studio.state == "finished"),
        origin: ReaderOrigin::Records(key),
        ..ReaderMeta::default()
    }
}

pub(super) fn studio_exchange_meta(
    machines: &[MachineSummary],
    key: StudioKey,
    seq: u64,
    fallback_file: Option<&str>,
    hw_terminal: Option<String>,
) -> ReaderMeta {
    let studio_ref = data::studio(machines, &key);
    let hw = hollywood_summary(machines);
    let mut meta = ReaderMeta {
        kind: ReaderKind::Outcome,
        studio: Some(key.id.clone()),
        machine: key.machine.clone(),
        seq: Some(seq),
        file: fallback_file.map(str::to_owned),
        origin: ReaderOrigin::StudioExchange(key, seq),
        ..ReaderMeta::default()
    };
    let Some(s) = studio_ref.map(|r| r.studio) else {
        return meta;
    };
    let Some((idx, entry)) = s.exchange.iter().enumerate().find(|(_, e)| e.seq == seq) else {
        return meta;
    };

    meta.kind = if entry.who == "user" {
        ReaderKind::Tell
    } else if entry.kind.as_deref() == Some("report") {
        ReaderKind::Report
    } else {
        ReaderKind::Outcome
    };
    meta.from = Some(entry.who.clone());
    meta.to = entry.to.clone();
    meta.task = entry.task.clone();
    meta.file = entry
        .file
        .clone()
        .or_else(|| fallback_file.map(str::to_owned));
    meta.ts_ms = entry.ts_ms;
    meta.nav_index = Some(idx + 1);
    meta.nav_total = Some(s.exchange.len());

    let lookup_role = if entry.who == "user" {
        entry.to.as_deref()
    } else {
        Some(entry.who.as_str())
    };
    if let Some(name) = lookup_role {
        if let Some(role) = find_studio_role(s, name) {
            meta.role = Some(role.role.clone());
            meta.role_state = Some(role_state_word(role).into());
            meta.role_terminal = (role.state == "live")
                .then(|| role.terminal.clone())
                .flatten();
            meta.attention_user = Person::of(role) == Person::UserPrompt;
            meta.runner = role.runner.clone();
            meta.model = role.model.clone();
            meta.resumable = role.resumable;
        } else if name == "hollywood"
            && let Some(h) = hw
        {
            meta.role = Some("hollywood".into());
            meta.role_state = Some(hollywood_state_word(h).into());
            meta.role_terminal = hw_terminal.filter(|_| h.state == "live");
            meta.attention_user = h.attention_user;
            meta.runner = h.runner.clone();
            meta.model = h.model.clone();
        }
    }
    meta
}
