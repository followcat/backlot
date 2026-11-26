// Derived from herdr src/app.rs (Apache-2.0).

//! The lot latch — what the tab-bar pill and the studio header read
//! each render. Recomputed on every `sync_studio` from the summary
//! document and the focused terminal's labels, so it can never
//! disagree with the panel the way the old floating strip did.

use crate::panel::data::MachineSummary;

use crate::chrome::lot::{HeaderKind, Lot, LotCounts, StudioHeader};
use crate::hollywood::LayerMode;
use crate::panel::{CallSheetStart, data};
use crate::project::LOCAL_LABEL;
use crate::studio::StudioKey;
use crate::studio::glyphs::{Person, PriorityMark};

impl super::App {
    /// Rebuild the lot latch — counts, the focused workspace's header
    /// and whether the pill shows. Called from `sync_studio` with the
    /// machine summaries it already collected.
    pub(super) fn sync_lot(&mut self, machines: &[MachineSummary]) {
        let prior = std::mem::take(&mut self.studio.lot);
        let mut lot = Lot {
            start_pending: prior.start_pending,
            start_applied: prior.start_applied,
            ..Lot::default()
        };
        if !self.plain {
            self.sync_hollywood_seen(machines);
            self.sync_handoff_outcome(machines);
            self.lot_counts(machines, &mut lot.counts);
            lot.pill = lot.counts.studios > 0
                || self.has_hollywood_cfg
                || self.panel_config.hollywood_table;
            lot.header = self.studio_header(machines);
            lot.studio_workspace = lot
                .header
                .as_ref()
                .is_some_and(|h| !matches!(h.kind, HeaderKind::Hollywood));
            self.lot_start_policy(&mut lot, prior.was_urgent);
            if self.hollywood.mode == LayerMode::Expanded {
                self.sync_hollywood_seen(machines);
                lot.counts.unread = 0;
            }
        }
        self.studio.lot = lot;
    }

    /// Seed `seen_hollywood_seq` from the first received summary
    /// document, and advance it whenever the Call Sheet is `Expanded`.
    pub(crate) fn sync_hollywood_seen(&mut self, machines: &[MachineSummary]) {
        let Some(local) = crate::hollywood::local_summary(machines) else {
            return;
        };
        let max_seq = crate::hollywood::hollywood_reply_max_seq(local.hollywood.as_ref());
        match self.hollywood.seen_hollywood_seq {
            None => {
                self.hollywood.seen_hollywood_seq = Some(max_seq);
            }
            Some(prev) if self.hollywood.mode == LayerMode::Expanded => {
                self.hollywood.seen_hollywood_seq = Some(prev.max(max_seq));
            }
            Some(_) => {}
        }
    }

    /// Compute the four Call Sheet urgency buckets from `waiting_items`, keeping
    /// the Lot Pill counts in sync with the sheet rows.
    fn lot_counts(&self, machines: &[MachineSummary], counts: &mut LotCounts) {
        let studios = data::studios(machines);
        counts.studios = studios.len();
        counts.activity = if self.has_hollywood_cfg {
            crate::hollywood::hollywood_activity(machines, self.hollywood.pending.as_deref())
        } else {
            crate::hollywood::HollywoodActivity::Idle
        };
        if self.hollywood.mode != LayerMode::Expanded {
            let seen = self.hollywood.seen_hollywood_seq.unwrap_or(0);
            let hw = crate::hollywood::local_summary(machines).and_then(|s| s.hollywood.as_ref());
            counts.unread = crate::hollywood::hollywood_unread_count(hw, seen);
        }
        let status = crate::hollywood::status_of(&self.view);
        for wait in &crate::hollywood::waiting_items(machines, &status) {
            match crate::hollywood::WaitBucket::of(wait) {
                crate::hollywood::WaitBucket::Blocking => counts.blocking += 1,
                crate::hollywood::WaitBucket::Asks => counts.asks += 1,
                crate::hollywood::WaitBucket::Cards => counts.cards += 1,
                crate::hollywood::WaitBucket::Ready => counts.ready += 1,
            }
        }
        let hw = crate::hollywood::local_summary(machines).and_then(|s| s.hollywood.as_ref());
        counts.switch_state = crate::hollywood::pill_switch_state(hw, counts.any_waiting());
    }

    fn sync_handoff_outcome(&mut self, machines: &[MachineSummary]) {
        let Some(hw) = crate::hollywood::local_summary(machines).and_then(|s| s.hollywood.as_ref())
        else {
            return;
        };
        let Some(hf) = hw.handoff.as_ref() else {
            return;
        };
        let next = (hf.id.clone(), hf.phase.clone());
        if self.last_handoff_seen.as_ref() == Some(&next) {
            return;
        }
        let prev_phase = self
            .last_handoff_seen
            .as_ref()
            .filter(|(id, _)| id == &hf.id)
            .map(|(_, p)| p.clone());
        self.last_handoff_seen = Some(next);
        match hf.phase.as_str() {
            "committed" if prev_phase.is_some() => {
                let target = crate::hollywood::handoff_target_short(hf);
                self.reload_hollywood_cfg();
                let updated = backlot_stage::config::HollywoodCfg {
                    runner: hf.target_runner.clone(),
                    model: hf.target_model.clone(),
                    effort: hf.target_effort.clone(),
                    key: self.hollywood_cfg.as_ref().and_then(|c| c.key.clone()),
                    default_cast: self
                        .hollywood_cfg
                        .as_ref()
                        .and_then(|c| c.default_cast.clone()),
                };
                self.has_hollywood_cfg = true;
                self.hollywood_overseer = Some(super::hollywood::overseer_label(&updated));
                self.hollywood_cfg = Some(updated);
                self.show_handoff_toast(&format!("switched hollywood to {target}"), "", false);
            }
            "rolled_back" => {
                let msg = crate::hollywood::rollback_banner(hw, hf);
                self.show_handoff_toast("hollywood switch failed", &msg, true);
            }
            _ => {}
        }
    }

    /// The start policy, applied once the first summary lands, plus
    /// the `when_waiting` rising edge while the bar sits collapsed.
    fn lot_start_policy(&mut self, lot: &mut Lot, was_urgent: bool) {
        let urgent = lot.counts.any_waiting() > 0;
        if lot.start_pending {
            if self.opening.ready() {
                lot.start_pending = false;
                lot.start_applied = true;
                let expand = match self.panel_config.call_sheet_start {
                    CallSheetStart::Expanded => true,
                    CallSheetStart::WhenWaiting => urgent,
                    CallSheetStart::Collapsed => false,
                };
                if expand && self.hollywood.mode == LayerMode::Collapsed {
                    self.hollywood_open();
                }
            }
            // Not ready yet — the policy stays pending.
        } else if matches!(
            self.panel_config.call_sheet_start,
            CallSheetStart::WhenWaiting
        ) && self.hollywood.mode == LayerMode::Collapsed
            && urgent
            && !was_urgent
        {
            self.hollywood_open();
        }
        lot.was_urgent = urgent;
    }

    /// The focused workspace's studio header, if the focused terminal
    /// is a studio role or shell — or `hollywood` under the
    /// Hollywood terminal. Any other workspace: `None`, the tab bar
    /// stays Herdr's.
    fn studio_header(&self, machines: &[MachineSummary]) -> Option<StudioHeader> {
        let focused = self.focused.as_deref()?;
        let terminal = self.view.inventory.get(focused)?;
        let labels = &terminal.labels;
        if matches!(
            labels.get("backlot.role").map(String::as_str),
            Some("hollywood" | "hollywood-handoff" | "hollywood-previous")
        ) || terminal
            .name
            .as_deref()
            .is_some_and(backlot_model::hollywood::is_hollywood_terminal_name)
        {
            return Some(StudioHeader {
                key: None,
                studio: String::new(),
                machine: None,
                studio_focused: false,
                kind: HeaderKind::Hollywood,
            });
        }
        // A role is known by the summary itself — an older stage's
        // terminals carry no `backlot.*` labels.
        if let Some((m, s, role)) = machines
            .iter()
            .flat_map(|m| {
                m.summary
                    .studios
                    .iter()
                    .flat_map(move |s| s.roles.iter().map(move |r| (m.machine.as_deref(), s, r)))
            })
            .find(|(_, _, r)| r.terminal.as_deref() == Some(focused))
        {
            let key = StudioKey {
                machine: m.map(str::to_string),
                id: s.id.clone(),
            };
            let studio_focused = self.studio.focused_studio() == Some(&key);
            let machine = self.header_machine(&key, machines);
            return Some(StudioHeader {
                key: Some(key),
                studio: s.id.clone(),
                machine,
                studio_focused,
                kind: HeaderKind::Role {
                    role_key: role.role.clone(),
                    name: role.name.clone(),
                    task: role.task.clone(),
                    mark: Some(Person::of(role)),
                    priority: role.priority.and_then(PriorityMark::of),
                    runner: role.runner.clone(),
                    model: role.model.clone(),
                    effort: role.effort.clone(),
                },
            });
        }
        let id = labels.get("backlot.studio")?.clone();
        let machine = focused.split('/').next().filter(|m| *m != focused);
        let key = StudioKey {
            machine: machine.map(str::to_string),
            id: id.clone(),
        };
        if let Some(n) = labels
            .get("backlot.shell")
            .and_then(|s| s.parse::<u32>().ok())
        {
            let studio_focused = self.studio.focused_studio() == Some(&key);
            let machine = self.header_machine(&key, machines);
            return Some(StudioHeader {
                key: Some(key),
                studio: id,
                machine,
                studio_focused,
                kind: HeaderKind::Shell(n),
            });
        }
        let kind = labels
            .get("backlot.role")
            .map(|name| HeaderKind::Role {
                role_key: name.clone(),
                name: name.clone(),
                task: labels.get("backlot.task").cloned(),
                mark: None,
                priority: None,
                runner: None,
                model: None,
                effort: None,
            })
            .unwrap_or(HeaderKind::Bare);
        let studio_focused = self.studio.focused_studio() == Some(&key);
        let machine = self.header_machine(&key, machines);
        Some(StudioHeader {
            key: Some(key),
            studio: id,
            machine,
            studio_focused,
            kind,
        })
    }

    /// `· host-b` — the header shows the machine only when the
    /// aggregate spans machines, like the cast sidebar's labels.
    fn header_machine(&self, key: &StudioKey, machines: &[MachineSummary]) -> Option<String> {
        data::spans_machines(machines).then(|| {
            key.machine
                .clone()
                .unwrap_or_else(|| LOCAL_LABEL.to_string())
        })
    }
}

impl super::App {
    /// W1b: what the lot's empty state should read when no document has a
    /// studio but a stopped machine's ledger lists some — the count, and
    /// the one machine holding them (so `s` can start its stage), `None`
    /// with several. `None` when any document already lists a studio (the
    /// lot is not empty then) or the ledger lists none either.
    pub(crate) fn stopped_ledger(
        &self,
        machines: &[MachineSummary],
    ) -> Option<crate::panel::StoppedLedger> {
        use crate::chrome::snapshot::ClientEndpointId;
        use crate::studio_index::LedgerState;
        if crate::panel::data::studio_count(machines) > 0 {
            return None;
        }
        let mut count = 0;
        let mut machine: Option<Option<String>> = None;
        let mut several = false;
        for endpoint in &self.projection.endpoints {
            let name = match &endpoint.endpoint_id {
                ClientEndpointId::Local => None,
                ClientEndpointId::Remote(name) => Some(name.clone()),
            };
            let listed = self
                .studio_index
                .studios(name.as_deref())
                .iter()
                .filter(|studio| studio.state != LedgerState::Finished)
                .count();
            if listed == 0 {
                continue;
            }
            count += listed;
            match &machine {
                None => machine = Some(name),
                Some(known) if *known == name => {}
                Some(_) => several = true,
            }
        }
        (count > 0).then_some(crate::panel::StoppedLedger {
            count,
            machine: if several { None } else { machine },
        })
    }
}
