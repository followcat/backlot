//! The Hollywood layer's App wiring (H4): the `[hollywood]` config
//! load, the `alt+y` toggle, the expanded panel's keys and mouse and
//! the `backlot` action runner. The row model and renderer live in
//! `crate::hollywood`; everything here only feeds them `Hollywood`,
//! the view's `backlot` document and the inventory's stage flag.
//!
//! The layer is a float: nothing here touches `chrome_layout`, so no
//! pane is ever resized for it.

mod keys;
mod mouse;

use std::path::Path;

use backlot_model::summary::Summary;

use super::App;
use crate::hollywood::{self, CardView, HollywoodKey, LayerMode, Target};

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
        // R22: the expanded studio panel names `hollywood setup` only
        // while no table exists.
        self.panel.hollywood_hint = !self.has_hollywood_cfg;
        match cfg {
            Some(cfg) => {
                let (key, key_notice) = hollywood::resolve_key(
                    cfg.key.as_deref(),
                    &self.panel_config.keys,
                    self.panel_key_combo,
                    path,
                );
                self.hollywood_key = Some(key);
                if let Some(key_notice) = key_notice {
                    self.show_notice("Hollywood config", &key_notice);
                }
                if self.hollywood.mode == LayerMode::Hidden && !self.plain {
                    self.hollywood.mode = LayerMode::Collapsed;
                }
            }
            None => {
                self.hollywood_key = None;
                self.hollywood.mode = LayerMode::Hidden;
            }
        }
        // U7: studio mode always has the bar; `--plain` never does.
        self.sync_hollywood_plain();
        if let Some(notice) = notice {
            self.show_notice("Hollywood config", &notice);
        }
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
            self.hollywood_key = Some(HollywoodKey::fallback());
            if self.hollywood.mode == LayerMode::Hidden {
                self.hollywood.mode = LayerMode::Collapsed;
            }
        }
    }

    /// Tests inject a recorder; production spawns `current_exe()`.
    pub(crate) fn set_hollywood_runner(&mut self, runner: hollywood::HollywoodRunner) {
        self.hollywood_runner = runner;
    }

    // ---------- data ----------

    /// The current `backlot` summary — the same read the panel makes.
    pub(crate) fn hollywood_summary(&self) -> Option<Summary> {
        crate::panel::summary(&self.view)
    }

    /// The row targets as they would draw now — selection works on
    /// stable identities, never positions.
    fn hollywood_targets(&self) -> Vec<Target> {
        let summary = self.hollywood_summary();
        hollywood::rows(
            summary.as_ref(),
            crate::panel::stage_running(&self.view),
            self.has_hollywood_cfg,
        )
        .iter()
        .map(|r| r.target())
        .collect()
    }

    /// The selected row's index in `targets` — a vanished target
    /// reverts to the title (index 0).
    fn hollywood_selected_index(&self, targets: &[Target]) -> usize {
        self.hollywood
            .selected
            .as_ref()
            .and_then(|t| targets.iter().position(|r| r == t))
            .unwrap_or(0)
    }

    // ---------- the toggle ----------

    /// The configured toggle (`alt+y` direct or its `[hollywood] key`
    /// override): hidden shows the strip, the strip expands, expanded
    /// collapses back to the strip.
    pub(crate) fn hollywood_toggle(&mut self) {
        self.hollywood.mode = match self.hollywood.mode {
            LayerMode::Hidden => LayerMode::Collapsed,
            LayerMode::Collapsed => LayerMode::Expanded,
            LayerMode::Expanded => LayerMode::Collapsed,
        };
        if self.hollywood.mode == LayerMode::Expanded {
            self.hollywood.selected = None;
            self.hollywood.scroll = 0;
        } else {
            // Leaving the panel retires its ephemeral sub-state.
            self.hollywood.view = None;
            self.hollywood.editing = false;
        }
    }

    /// Whether the press is the configured toggle — a direct binding
    /// matched like a studio key, or the chord tail while the prefix
    /// is armed. `None` without `[hollywood]` — the key is not ours.
    pub(crate) fn hollywood_key_press(&self, key: &super::input::TerminalKey) -> bool {
        // U7: no layer until the first read settled — the key is not
        // ours while it is on the wire.
        if !self.opening.ready() {
            return false;
        }
        let matched = crate::input::terminal_key_of(key);
        match self.hollywood_key {
            Some(HollywoodKey::Direct(combo)) => {
                crate::config::terminal_key_matches_combo(&matched, combo)
            }
            Some(HollywoodKey::Chord(combo)) => {
                self.prefix.armed() && crate::config::terminal_key_matches_combo(&matched, combo)
            }
            None => false,
        }
    }

    // ---------- rows / selection ----------

    /// j/k/arrows and paging: move the selection by `delta` rows and
    /// keep it visible, like `panel_move`.
    fn hollywood_move(&mut self, delta: isize) {
        let targets = self.hollywood_targets();
        if targets.len() <= 1 {
            return;
        }
        let mut index = self.hollywood_selected_index(&targets);
        for _ in 0..delta.unsigned_abs() {
            let next = if delta > 0 {
                (index + 1..targets.len()).find(|&i| targets[i] != targets[index])
            } else {
                (0..index).rev().find(|&i| targets[i] != targets[index])
            };
            match next {
                Some(next) => index = next,
                None => break,
            }
        }
        self.hollywood.selected = Some(targets[index].clone());
        // Keep the selection inside the scrolled body (title and the
        // input line are fixed rows 0 and last).
        let body_height = self.hollywood_hits.body.height.max(1);
        let body_rows = targets.len().saturating_sub(2);
        let heights: Vec<u16> = vec![1; body_rows];
        self.hollywood.scroll = crate::chrome::scroll::list_scroll_start_to_reveal(
            &heights,
            &[],
            body_height,
            self.hollywood.scroll,
            index.saturating_sub(1).min(body_rows.saturating_sub(1)),
        );
    }

    /// Wheel/drag scroll: clamp the offset into the drawn metrics.
    fn hollywood_scroll_to(&mut self, start: usize) {
        let max = self
            .hollywood_hits
            .metrics
            .map(|m| m.max_offset_from_bottom)
            .unwrap_or(0);
        self.hollywood.scroll = start.min(max);
    }

    /// Leave the layer after a row focused a terminal: the panel
    /// collapses back to its strip so the focused pane stays visible.
    fn hollywood_leave(&mut self) {
        self.hollywood.mode = LayerMode::Collapsed;
        self.hollywood.editing = false;
        self.hollywood.view = None;
    }

    // ---------- activation ----------

    /// Enter/left-click on a row: the title focuses `bl-hollywood`,
    /// a studio row focuses that studio through the panel's own path,
    /// a card row is its "Show me", the input line opens for editing.
    pub(super) fn hollywood_activate(&mut self) {
        let targets = self.hollywood_targets();
        let index = self.hollywood_selected_index(&targets);
        match targets.get(index).cloned() {
            Some(Target::Title) => self.hollywood_focus_terminal(),
            Some(Target::Studio(id)) => {
                self.focus_studio(&id);
                self.hollywood_leave();
            }
            Some(Target::Card(id)) => self.hollywood_card_show(&id),
            Some(Target::Input) => self.hollywood.editing = true,
            _ => {}
        }
    }

    /// Focus the `bl-hollywood` terminal — same read `reap` and
    /// `stage_running` make of the inventory: the terminal named so.
    fn hollywood_focus_terminal(&mut self) {
        let terminal = self
            .view
            .inventory
            .terminals()
            .values()
            .find(|t| t.name.as_deref() == Some(backlot_model::hollywood::HOLLYWOOD_TERMINAL))
            .map(|t| t.address.clone());
        match terminal {
            Some(id) => {
                self.focus_pane(id);
                self.hollywood_leave();
            }
            None => self.show_notice(
                "Hollywood",
                "bl-hollywood is not running — the stage starts it",
            ),
        }
    }

    // ---------- card actions ----------

    /// The argv tail for a layer command — the subcommand first,
    /// then `--as user` and the endpoint flags, like `panel_argv`.
    fn hollywood_argv(&self, command: &[String]) -> Vec<String> {
        let mut argv = command.to_vec();
        argv.extend(["--as".into(), "user".into()]);
        argv.extend(self.panel_endpoint_args.iter().cloned());
        argv
    }

    /// `a` / Apply on the selected card: `backlot card apply <id>`.
    fn hollywood_card_apply(&mut self, id: &str) {
        let argv = self.hollywood_argv(&["card".into(), "apply".into(), id.to_string()]);
        self.run_hollywood_action(hollywood::HollywoodAction {
            label: format!("card apply {id}"),
            argv,
            view: false,
        });
    }

    /// `c` / Cancel on the selected card: `backlot card cancel <id>`
    /// with the layer's reason.
    fn hollywood_card_cancel(&mut self, id: &str) {
        let argv = self.hollywood_argv(&[
            "card".into(),
            "cancel".into(),
            id.to_string(),
            "--reason".into(),
            "cancelled from the layer".into(),
        ]);
        self.run_hollywood_action(hollywood::HollywoodAction {
            label: format!("card cancel {id}"),
            argv,
            view: false,
        });
    }

    /// `s` / Show me: a `studio-new` card's draft in the read-only
    /// view (`backlot card show <id>`); any other card focuses the
    /// studio it names.
    fn hollywood_card_show(&mut self, id: &str) {
        let summary = self.hollywood_summary();
        let card = summary
            .as_ref()
            .and_then(|s| s.hollywood.as_ref())
            .and_then(|h| h.cards.iter().find(|c| c.id == id));
        let Some(card) = card.cloned() else { return };
        if card.action.starts_with("studio-new") {
            let argv = self.hollywood_argv(&["card".into(), "show".into(), id.to_string()]);
            self.run_hollywood_action(hollywood::HollywoodAction {
                label: format!("card show {id}"),
                argv,
                view: true,
            });
        } else {
            self.focus_studio(&card.studio);
            self.hollywood_leave();
        }
    }

    /// The key/mouse entry for a card target's button kind.
    pub(super) fn hollywood_card_button(&mut self, btn: &hollywood::CardBtn) {
        match btn.kind {
            hollywood::BtnKind::Apply => self.hollywood_card_apply(&btn.card),
            hollywood::BtnKind::Cancel => self.hollywood_card_cancel(&btn.card),
            hollywood::BtnKind::Show => self.hollywood_card_show(&btn.card),
        }
    }

    /// The selected card's Apply/Cancel/Show — `a`, `c`, `s` keys.
    pub(super) fn hollywood_card_key(&mut self, kind: hollywood::BtnKind) {
        let targets = self.hollywood_targets();
        let index = self.hollywood_selected_index(&targets);
        let Some(Target::Card(id)) = targets.get(index).cloned() else {
            return;
        };
        self.hollywood_card_button(&hollywood::CardBtn { card: id, kind });
    }

    // ---------- input line ----------

    /// Enter on the editing input line: the editor closes — a
    /// nonempty line goes out as `backlot tell hollywood "<line>"`
    /// (D20); an empty one sends nothing.
    fn hollywood_input_submit(&mut self) {
        self.hollywood.editing = false;
        let line = self.hollywood.input.as_str().trim().to_string();
        if line.is_empty() {
            return;
        }
        self.hollywood.input.clear();
        let argv = self.hollywood_argv(&["tell".into(), "hollywood".into(), line]);
        self.run_hollywood_action(hollywood::HollywoodAction {
            label: "tell hollywood".into(),
            argv,
            view: false,
        });
    }

    // ---------- the action runner ----------

    /// Spawn the action off the event loop through the injected
    /// runner; the result comes back on `hwood_rx`.
    fn run_hollywood_action(&mut self, action: hollywood::HollywoodAction) {
        let future = (self.hollywood_runner)(action.argv);
        let tx = self.hwood_tx.clone();
        tokio::spawn(async move {
            let _ = tx.send((action.label, future.await, action.view)).await;
        });
    }

    /// The action's result: `card show` output opens the view; a
    /// failure is a notice carrying the first stderr line.
    pub(crate) fn on_hollywood_action(
        &mut self,
        (label, result, view): (String, Result<String, String>, bool),
    ) {
        match result {
            Ok(stdout) if view => {
                let title = label.trim_start_matches("card show ").to_string();
                let lines: Vec<String> = stdout.lines().map(str::to_string).collect();
                if lines.is_empty() {
                    self.show_notice("Hollywood", &format!("{label}: nothing to show"));
                } else {
                    self.hollywood.view = Some(CardView {
                        title,
                        lines,
                        scroll: 0,
                    });
                }
            }
            Ok(_) => {}
            Err(error) => {
                self.show_notice("Hollywood action failed", &format!("{label}: {error}"));
            }
        }
    }

    // ---------- the "Show me" view ----------

    /// The open view owns the layer's input: Esc/x/q/Enter close,
    /// arrows scroll, everything else is eaten.
    fn hollywood_view_key(&mut self, key: &super::input::TerminalKey) {
        let combo = crate::input::terminal_key_of(key);
        let (code, modifiers) = crate::config::normalize_key_combo((combo.code, combo.modifiers));
        let plain = modifiers.is_empty();
        let Some(view) = self.hollywood.view.as_mut() else {
            return;
        };
        match code {
            crossterm::event::KeyCode::Esc
            | crossterm::event::KeyCode::Enter
            | crossterm::event::KeyCode::Char('x')
            | crossterm::event::KeyCode::Char('q')
                if plain =>
            {
                self.hollywood.view = None;
            }
            crossterm::event::KeyCode::Down | crossterm::event::KeyCode::Char('j') if plain => {
                view.scroll = view.scroll.saturating_add(1);
            }
            crossterm::event::KeyCode::Up | crossterm::event::KeyCode::Char('k') if plain => {
                view.scroll = view.scroll.saturating_sub(1);
            }
            _ => {}
        }
        // Clamp into the last drawn metrics so the wheel cannot run
        // the scroll past the end.
        if let (Some(view), Some(metrics)) = (
            self.hollywood.view.as_mut(),
            self.hollywood_hits.view.as_ref().and_then(|v| v.metrics),
        ) {
            view.scroll = view.scroll.min(metrics.max_offset_from_bottom);
        }
    }

    /// The view's mouse: the wheel scrolls, a press outside closes,
    /// inside is eaten.
    fn hollywood_view_mouse(&mut self, mouse: &super::input::MouseInput) -> bool {
        let Some(hits) = self.hollywood_hits.view.as_ref() else {
            return false;
        };
        let point = (mouse.col, mouse.row);
        use super::input::{MouseButton, MouseEventKind};
        match mouse.kind {
            MouseEventKind::ScrollUp => {
                if let Some(view) = self.hollywood.view.as_mut() {
                    view.scroll = view.scroll.saturating_sub(1);
                }
            }
            MouseEventKind::ScrollDown => {
                if let (Some(view), Some(metrics)) = (self.hollywood.view.as_mut(), hits.metrics) {
                    view.scroll = (view.scroll + 1).min(metrics.max_offset_from_bottom);
                }
            }
            MouseEventKind::Down(MouseButton::Left) if !hits.area.contains(point.into()) => {
                self.hollywood.view = None;
            }
            _ => {}
        }
        true
    }
}
