//! The Hollywood layer's App wiring (H4): the `[hollywood]` config
//! load, the `alt+y` toggle, the expanded panel's keys and mouse and
//! the `backlot` action runner. The row model and renderer live in
//! `crate::hollywood`; everything here only feeds them `Hollywood`,
//! the view's `backlot` document and the inventory's stage flag.
//!
//! The layer is a float: nothing here touches `chrome_layout`, so no
//! pane is ever resized for it.

mod actions;
mod activate;
mod keys;
mod mouse;
mod nav;
mod reader_meta_from;
mod reader_nav;
mod view;

use std::path::Path;

use super::App;
use crate::hollywood::{self, CallSheetKeys, LayerMode, Section, Target};

/// The overseer row's `runner / model / effort` — the Call Sheet
/// title's own join; `effort` drops when unset or `auto` (R3-M15).
pub(super) fn overseer_label(cfg: &backlot_stage::config::HollywoodCfg) -> String {
    [
        Some(cfg.runner.as_str()),
        Some(cfg.model.as_str()),
        cfg.effort.as_deref().filter(|effort| *effort != "auto"),
    ]
    .into_iter()
    .flatten()
    .collect::<Vec<_>>()
    .join(" / ")
}

impl App {
    // ---------- config ----------

    /// cli injects the `[hollywood]` read: `Ok(Some)` configures the
    /// agent and starts the layer; `Ok(None)` still leaves the bar in
    /// studio mode — in its no-agent form (U7) — with the default
    /// toggle; under `--plain` there is no layer at all, so Alt+Y
    /// reaches the program exactly as before (D6, D15a); `Err` is a
    /// malformed file or section, noticed once with the position only.
    pub(crate) fn set_hollywood(
        &mut self,
        read: Result<Option<backlot_stage::config::HollywoodCfg>, String>,
        path: &Path,
    ) {
        let (cfg, notice) = match read {
            Ok(cfg) => (cfg, None),
            Err(e) => (None, Some(format!("Hollywood config: {e}"))),
        };
        self.has_hollywood_cfg = cfg.is_some();
        // The settings tab's read-only overseer row — the Call Sheet
        // title's own `runner / model / effort` join (R3-M15).
        self.hollywood_overseer = cfg.as_ref().map(overseer_label);
        // The reconfigure dialog's prefill — `key`/`default_cast`
        // carry over verbatim on its write.
        self.hollywood_cfg = cfg.clone();
        // R22: the expanded studio panel names `hollywood setup` only
        // while no table exists.
        self.panel.hollywood_hint = !self.has_hollywood_cfg;
        // The toggle resolves from the same inputs either way:
        // `[keys] call_sheet`/`prefix_call_sheet`, then `[hollywood]
        // key` for whichever slots those leave open — read leniently
        // from the panel config so a client-only table still counts.
        let raw_key = cfg
            .as_ref()
            .and_then(|c| c.key.as_deref())
            .or(self.panel_config.hollywood_key.as_deref());
        let (key, key_notices) = hollywood::resolve_toggle(
            self.panel_config.keys_call_sheet.as_deref(),
            self.panel_config.keys_prefix_call_sheet.as_deref(),
            raw_key,
            &self.panel_config.keys,
            self.panel_key_combo,
            path,
        );
        self.hollywood_key = Some(key);
        for key_notice in key_notices {
            self.show_notice("Hollywood config", &key_notice);
        }
        // U7: studio mode always has the bar — configured or not —
        // collapsed; `--plain` hides it in `sync_hollywood_plain`.
        if self.hollywood.mode == LayerMode::Hidden && !self.plain {
            self.hollywood.mode = LayerMode::Collapsed;
        }
        // The start policy applies once on the first publish — or
        // right now when the read lands after the settle already ran.
        if !self.studio.lot.start_applied {
            self.studio.lot.start_pending = true;
            if self.opening.ready() && !self.plain {
                let machines = self.panel_summaries();
                self.sync_lot(&machines);
            }
        }
        // U7: studio mode always has the bar; `--plain` never does.
        self.sync_hollywood_plain();
        if let Some(notice) = notice {
            self.show_notice("Hollywood config", &notice);
        }
    }

    /// cli hands over the detector's first runner — the `<r>` in the
    /// overseer row's setup hint while `[hollywood]` is unset.
    pub(crate) fn set_setup_runner_hint(&mut self, runner: Option<String>) {
        self.setup_runner_hint = runner;
    }

    /// R3-M15: `Ctrl+B s` re-reads `[hollywood]` from the file
    /// `panel_config_path` names — an edit made since start lands in
    /// the studio tab without a restart. The same `set_hollywood`
    /// install the startup read takes.
    pub(super) fn reload_hollywood_cfg(&mut self) {
        let path = self.panel_config_path.clone();
        self.set_hollywood(backlot_stage::config::hollywood_at(&path), &path);
    }

    /// The `--plain` latch on the layer (D6, D15a): plain has no
    /// layer, no consumed key and no timer, so the frame stays
    /// herdr's. Studio mode always has the bar — configured or not —
    /// and its toggle is the default `alt+y` unless `[hollywood] key`
    /// replaced it.
    pub(crate) fn sync_hollywood_plain(&mut self) {
        if self.plain {
            self.hollywood_key = None;
            self.hollywood.mode = LayerMode::Hidden;
            self.hollywood.editing = false;
            self.hollywood.view = None;
        } else if self.hollywood_key.is_none() {
            self.hollywood_key = Some(CallSheetKeys {
                direct: crate::config::parse_key_combo("alt+y"),
                chord: crate::config::parse_key_combo("i"),
            });
            if self.hollywood.mode == LayerMode::Hidden {
                self.hollywood.mode = LayerMode::Collapsed;
            }
        }
    }

    // ---------- data ----------

    /// The frames' targets in aggregate order — every machine's
    /// studios, then the new-studio entry.
    pub(super) fn hollywood_frames(&self) -> Vec<Target> {
        let machines = self.panel_summaries();
        hollywood::frames(&machines)
            .iter()
            .map(|f| f.target())
            .collect()
    }

    /// The Call Sheet's waiting items — every machine's, in urgency
    /// order.
    fn hollywood_waits(&self) -> Vec<Target> {
        let machines = self.panel_summaries();
        let status = hollywood::status_of(&self.view);
        hollywood::waiting_items(&machines, &status)
            .iter()
            .map(|w| w.target())
            .collect()
    }

    /// The band's targets in draw order: the headline, the waiting
    /// items (or the calm row), then the input/setup line.
    pub(super) fn hollywood_band(&self) -> Vec<Target> {
        let machines = self.panel_summaries();
        let status = hollywood::status_of(&self.view);
        let (waits, rows) = hollywood::band_rows(&machines, &status, self.has_hollywood_cfg);
        rows.iter().map(|r| r.target(&waits)).collect()
    }

    /// Every actionable target: the title row, the frames, then the
    /// band's rows — selection works on stable identities, never
    /// positions.
    pub(crate) fn hollywood_targets(&self) -> Vec<Target> {
        let mut targets = vec![Target::Title];
        targets.extend(self.hollywood_frames());
        targets.extend(self.hollywood_band());
        targets
    }

    /// The selected target's index in `targets` — a vanished target
    /// reverts to the first frame, not the title: the title is only
    /// the lot's face, and actions meant for the row the user last
    /// saw highlighted must not land on it silently.
    pub(super) fn hollywood_selected_index(&self, targets: &[Target]) -> usize {
        self.hollywood
            .selected
            .as_ref()
            .and_then(|t| targets.iter().position(|r| r == t))
            .unwrap_or_else(|| {
                targets
                    .iter()
                    .position(|t| matches!(t, Target::Studio(_) | Target::NewStudio))
                    .unwrap_or(0)
            })
    }

    /// How many frames the last draw showed, and how many per row.
    pub(super) fn hollywood_frame_window(&self) -> (usize, usize) {
        let rects: Vec<ratatui::layout::Rect> = self
            .hollywood_hits
            .rows
            .iter()
            .filter(|(_, target)| matches!(target, Target::Studio(_) | Target::NewStudio))
            .map(|(rect, _)| *rect)
            .collect();
        let Some(first) = rects.first() else {
            return (0, 1);
        };
        let per_row = rects.iter().filter(|rect| rect.y == first.y).count().max(1);
        (rects.len(), per_row)
    }

    // ---------- the toggle ----------

    /// Expand the sheet into `Compose` on `Target::Input` when `[hollywood]`
    /// is set (waiting items remain visible above), or keep wait/frame
    /// landing when unconfigured. Preserves any parked `input` draft.
    fn hollywood_enter_expanded(&mut self) {
        self.hollywood.mode = LayerMode::Expanded;
        self.hollywood.chip_index = None;
        self.hollywood.last_upper = None;
        self.hollywood.last_lower = None;
        self.hollywood.frame_scroll = 0;
        self.hollywood.scroll = 0;
        self.hollywood.history.reset_nav();
        if self.has_hollywood_cfg {
            self.hollywood.selected = Some(Target::Input);
            self.hollywood.editing = true;
        } else {
            self.hollywood.selected = self.hollywood_initial_target();
            self.hollywood.editing = false;
        }
        self.hollywood_remember_selected();
        let machines = self.panel_summaries();
        self.hollywood_sync_history_from(&machines);
        self.sync_hollywood_seen(&machines);
        self.studio.lot.counts.unread = 0;
    }

    /// Open the bar expanded — the toggle's open arm, the start
    /// policy's `expanded`/`when_waiting` branches, menu and help share it.
    pub(crate) fn hollywood_open(&mut self) {
        self.hollywood_enter_expanded();
    }

    /// Sync `hollywood.history` from `HollywoodSummary.exchange`.
    pub(crate) fn sync_hollywood_history(&mut self) {
        let machines = self.panel_summaries();
        self.hollywood_sync_history_from(&machines);
    }

    pub(crate) fn hollywood_sync_history_from(
        &mut self,
        machines: &[crate::panel::data::MachineSummary],
    ) {
        let exchange = hollywood::local_summary(machines)
            .and_then(|s| s.hollywood.as_ref())
            .map(|h| h.exchange.as_slice())
            .unwrap_or(&[]);
        self.hollywood.history.sync(
            exchange
                .iter()
                .map(|e| (e.seq, e.who.as_str(), e.text.as_str())),
        );
    }

    /// The `Compose` latch can only live on the `›` row — every
    /// selection change ends in this clamp so walking onto or past
    /// the line rests it (`Resting`, draft kept) instead of
    /// re-arming the edit (R3-B2). Only `Enter`/`i`/a click on the
    /// row arms `Compose` again.
    pub(super) fn hollywood_sync_editing(&mut self) {
        self.hollywood.editing &= self.hollywood.selected == Some(Target::Input);
        if !self.hollywood.editing {
            self.hollywood.history.reset_nav();
        }
    }

    /// What an opening selects when `[hollywood]` is absent: the first
    /// waiting item, else the focused studio's frame, else the first frame.
    fn hollywood_initial_target(&self) -> Option<Target> {
        if let Some(first) = self.hollywood_waits().into_iter().next() {
            return Some(first);
        }
        let frames = self.hollywood_frames();
        self.studio
            .current
            .as_ref()
            .and_then(|key| {
                frames
                    .iter()
                    .find(|t| **t == Target::Studio(key.clone()))
                    .cloned()
            })
            .or_else(|| frames.first().cloned())
    }

    /// The configured toggle (`alt+y`, `Ctrl+B i`, clicking the pill):
    /// `Hidden | Collapsed => Expanded` (straight into Compose when
    /// `[hollywood]` is set; unconfigured keeps wait/frame landing), and
    /// `Expanded => Collapsed` keeping any draft.
    pub(crate) fn hollywood_toggle(&mut self) {
        if self.hollywood.mode == LayerMode::Expanded {
            self.hollywood_collapse();
            let machines = self.panel_summaries();
            self.sync_lot(&machines);
        } else {
            self.hollywood_enter_expanded();
        }
    }

    /// Whether the press is the configured toggle — the direct
    /// binding matched like a studio key, or the chord tail while the
    /// prefix is armed. `None` without a table — the key is not ours.
    pub(crate) fn hollywood_key_press(&self, key: &super::input::TerminalKey) -> bool {
        // U7: no layer until the first read settled — the key is not
        // ours while it is on the wire.
        if !self.opening.ready() {
            return false;
        }
        let matched = crate::input::terminal_key_of(key);
        self.hollywood_key
            .is_some_and(|keys| keys.press(&matched, self.prefix.armed()))
    }

    /// Sync `section` and `last_upper`/`last_lower` from the current `selected`.
    pub(super) fn hollywood_remember_selected(&mut self) {
        if let Some(sel) = self.hollywood.selected.clone() {
            let sec = Section::of(&sel);
            self.hollywood.section = sec;
            match sec {
                Section::Upper if !matches!(sel, Target::Title) => {
                    self.hollywood.last_upper = Some(sel);
                }
                Section::Lower => self.hollywood.last_lower = Some(sel),
                _ => {}
            }
        }
    }
}
