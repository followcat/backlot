//! Reader metadata resolution, sibling stepping (`←`/`→`, `[`/`]`),
//! and live Lot Dock click switching while keeping the Reader open.

use backlot_model::summary::StudioExchange;

use super::App;
use super::reader_meta_from::{
    card_meta, hollywood_base_meta, hollywood_exchange_meta, hollywood_summary, records_meta,
    studio_exchange_meta,
};
use crate::action::Action;
use crate::hollywood::{CardView, ReaderKind, ReaderMeta, ReaderOrigin, Target};
use crate::panel::data::{self, MachineSummary};
use crate::studio::StudioKey;
use crate::studio_cmds;

impl App {
    /// Apply a completed `view: true` action reply to the open Reader.
    pub(crate) fn on_view_action_reply(&mut self, action: &Action, stdout: &str) {
        let card = action
            .mark
            .strip_prefix("card:")
            .or_else(|| action.label.strip_prefix("card show "))
            .map(str::to_owned)
            .or_else(|| card_show_id(action));
        let title = match &card {
            Some(id) => id.clone(),
            None => match &action.machine {
                Some(machine) => format!("{} · {machine}", action.name),
                None => action.name.clone(),
            },
        };
        let meta = self.reader_meta_for_action(action);
        if self
            .hollywood
            .view
            .as_ref()
            .and_then(|v| v.meta.loading_origin.as_ref())
            .is_some_and(|exp| *exp != meta.origin)
        {
            return;
        }
        let lines: Vec<String> = stdout.lines().map(str::to_string).collect();
        if lines.is_empty() {
            self.reader_clear_loading();
            self.show_notice("Hollywood", &format!("{title}: nothing to show"));
        } else {
            self.hollywood.view = Some(CardView {
                card,
                title,
                lines,
                raw_body: stdout.to_owned(),
                scroll: 0,
                meta,
                ..CardView::default()
            });
        }
    }

    /// Derive structured [`ReaderMeta`] for a completed `view: true` action reply.
    pub(crate) fn reader_meta_for_action(&self, action: &Action) -> ReaderMeta {
        let machines = self.panel_summaries();
        let hw_term = self.hollywood_live_terminal();
        if let Some(card_id) = action
            .mark
            .strip_prefix("card:")
            .or_else(|| action.label.strip_prefix("card show "))
        {
            return card_meta(&machines, card_id);
        }
        if let Some(seq) = action
            .name
            .strip_prefix("hollywood #")
            .and_then(|s| s.parse::<u64>().ok())
        {
            return hollywood_exchange_meta(hollywood_summary(&machines), hw_term, seq);
        }
        if let Some(id) = action.name.strip_prefix("records ") {
            let key = StudioKey {
                machine: action.machine.clone(),
                id: id.to_string(),
            };
            return records_meta(&machines, key);
        }
        if let Some((id, seq_str)) = action.name.rsplit_once(" #")
            && let Ok(seq) = seq_str.parse::<u64>()
        {
            let key = StudioKey {
                machine: action.machine.clone(),
                id: id.to_string(),
            };
            let arg_file = action
                .argv
                .windows(2)
                .find(|w| w[0] == "--file")
                .map(|w| w[1].clone());
            return studio_exchange_meta(&machines, key, seq, arg_file.as_deref(), hw_term);
        }
        ReaderMeta::default()
    }

    /// Build metadata for an inline-only `Target::Headline` fallback (no `headline_seq`).
    pub(super) fn reader_meta_for_headline(&self, machines: &[MachineSummary]) -> ReaderMeta {
        let hw = hollywood_summary(machines);
        let mut meta = hollywood_base_meta(hw, self.hollywood_live_terminal());
        meta.kind = ReaderKind::Report;
        meta.studio = Some("hollywood".into());
        meta.from = Some("hollywood".into());
        meta.to = Some("user".into());
        meta.origin = hw
            .and_then(|h| h.headline_seq)
            .map(ReaderOrigin::HollywoodExchange)
            .unwrap_or_default();
        meta
    }

    /// Clear `loading_label`/`loading_origin` if a stepped async fetch fails.
    pub(crate) fn reader_clear_loading(&mut self) {
        if let Some(view) = self.hollywood.view.as_mut() {
            view.meta.loading_label = None;
            view.meta.loading_origin = None;
        }
    }

    /// Step backward (`delta = -1`) or forward (`delta = 1`) through the
    /// open Reader entry's sibling list (`StudioExchange`, `HollywoodExchange`, `Card`).
    pub(super) fn reader_step(&mut self, delta: isize) {
        let Some(origin) = self.hollywood.view.as_ref().map(|v| v.meta.origin.clone()) else {
            return;
        };
        let machines = self.panel_summaries();
        match origin {
            ReaderOrigin::StudioExchange(key, seq) => {
                let Some(studio) = data::studio(&machines, &key) else {
                    return;
                };
                let ex = &studio.studio.exchange;
                let Some(pos) = ex.iter().position(|e| e.seq == seq) else {
                    return;
                };
                let Some(next) = step_slice(ex, pos, delta) else {
                    return;
                };
                self.panel_focus_target(&crate::panel::Target::Talk(key.clone(), next.seq));
                self.open_studio_exchange_entry(&machines, &key, next);
            }
            ReaderOrigin::HollywoodExchange(seq) => {
                let Some(hw) = hollywood_summary(&machines) else {
                    return;
                };
                let Some(pos) = hw.exchange.iter().position(|e| e.seq == seq) else {
                    return;
                };
                let Some(next) = step_slice(&hw.exchange, pos, delta) else {
                    return;
                };
                let band = self.hollywood_band();
                let target = if band.contains(&Target::Talk(next.seq)) {
                    Target::Talk(next.seq)
                } else if hw.headline_seq == Some(next.seq) && band.contains(&Target::Headline) {
                    Target::Headline
                } else {
                    Target::Talk(next.seq)
                };
                self.hollywood_select(target.clone());
                if let Some(idx) = band.iter().position(|t| t == &target) {
                    self.hollywood_reveal_card(&band, idx);
                }
                let next_origin = ReaderOrigin::HollywoodExchange(next.seq);
                self.set_reader_loading(format!("#{}", next.seq), next_origin);
                let card = self.hollywood_exchange_is_card(next.seq);
                self.run_action(studio_cmds::hollywood_exchange(next.seq, card));
            }
            ReaderOrigin::Card(id) => {
                let Some(hw) = hollywood_summary(&machines) else {
                    return;
                };
                let Some(pos) = hw.cards.iter().position(|c| c.id == id) else {
                    return;
                };
                let Some(next) = step_slice(&hw.cards, pos, delta) else {
                    return;
                };
                let target = Target::Card(next.id.clone());
                let band = self.hollywood_band();
                self.hollywood_select(target.clone());
                if let Some(idx) = band.iter().position(|t| t == &target) {
                    self.hollywood_reveal_card(&band, idx);
                }
                let next_origin = ReaderOrigin::Card(next.id.clone());
                self.set_reader_loading(next.id.clone(), next_origin);
                self.run_action(studio_cmds::card_show_on(None, &next.id));
            }
            ReaderOrigin::Records(_) | ReaderOrigin::SheetDeliveries(_, _) | ReaderOrigin::None => {
            }
        }
    }

    /// Switch the open Reader in place to a sibling `Target::Talk(key, seq)` row
    /// clicked in the undimmed beside Lot Dock without closing.
    pub(super) fn reader_switch_dock_talk(&mut self, key: &StudioKey, seq: u64) -> bool {
        let machines = self.panel_summaries();
        let Some(studio) = data::studio(&machines, key) else {
            return false;
        };
        let Some(entry) = studio.studio.exchange.iter().find(|e| e.seq == seq) else {
            return false;
        };
        self.panel_focus_target(&crate::panel::Target::Talk(key.clone(), seq));
        self.open_studio_exchange_entry(&machines, key, entry);
        true
    }

    fn set_reader_loading(&mut self, label: String, origin: ReaderOrigin) {
        if let Some(view) = self.hollywood.view.as_mut() {
            view.meta.loading_label = Some(label);
            view.meta.loading_origin = Some(origin);
        }
    }

    fn open_studio_exchange_entry(
        &mut self,
        machines: &[MachineSummary],
        key: &StudioKey,
        entry: &StudioExchange,
    ) {
        if let Some(rel) = entry.file.as_deref() {
            let next_origin = ReaderOrigin::StudioExchange(key.clone(), entry.seq);
            self.set_reader_loading(format!("#{}", entry.seq), next_origin);
            self.run_action(studio_cmds::report_show(key, entry.seq, rel));
            return;
        }
        let mut lines: Vec<String> = entry.text.lines().map(str::to_string).collect();
        if lines.is_empty() {
            lines.push(String::new());
        }
        let title = match &key.machine {
            Some(m) => format!("{} #{} · {m}", key.id, entry.seq),
            None => format!("{} #{}", key.id, entry.seq),
        };
        let meta = studio_exchange_meta(
            machines,
            key.clone(),
            entry.seq,
            None,
            self.hollywood_live_terminal(),
        );
        self.hollywood.view = Some(CardView {
            card: None,
            title,
            lines,
            raw_body: entry.text.clone(),
            scroll: 0,
            meta,
            ..CardView::default()
        });
    }
}

/// The card id a `card show <id>` action reads, from its argv — the
/// `hollywood show`-shaped action carries the id nowhere else.
fn card_show_id(action: &Action) -> Option<String> {
    let argv = &action.argv;
    let argv: Vec<&str> = argv.iter().map(String::as_str).collect();
    let pos = argv.windows(2).position(|w| w == ["card", "show"])?;
    argv.get(pos + 2).map(|s| s.to_string())
}

fn step_slice<T>(items: &[T], pos: usize, delta: isize) -> Option<&T> {
    if delta < 0 {
        pos.checked_sub(delta.unsigned_abs())
            .and_then(|i| items.get(i))
    } else if delta > 0 {
        pos.checked_add(delta as usize).and_then(|i| items.get(i))
    } else {
        None
    }
}
