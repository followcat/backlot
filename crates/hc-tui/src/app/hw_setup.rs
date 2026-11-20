//! The Hollywood setup dialog's App wiring: every entry point —
//! the no-agent band's setup row, the panel's `hollywood setup`
//! hint, `c` on a configured bar, the settings overseer row — lands
//! on `hw_setup_open`. The dialog owns keys, mouse and IME text
//! while it is open like New Studio does; its one probe rides the
//! `NsRun` seam and its apply is a plain `run_action` — the stage's
//! own `hollywood setup` writes the table, never a second TOML path.

use hc_vt::{Key, Mods};

use super::new_studio::NsReply;
use super::{App, input::TerminalKey};
use crate::app::input::{MouseButton, MouseEventKind, MouseInput};
use crate::app::mouse::contains;
use crate::hw_setup::{Field, HwHit, HwSetup};
use crate::new_studio::{self, Probe};

impl App {
    /// Open the dialog — no machine choice: `hollywood setup` is a
    /// user command on this core, so the probe is always the local
    /// `runners --json`. An existing `[hollywood]` prefills the
    /// rows; a reconfigure forwards `key`/`default_cast` verbatim.
    pub(crate) fn hw_setup_open(&mut self) {
        if self.plain {
            return;
        }
        self.hw_setup = Some(HwSetup::new(self.hollywood_cfg.clone()));
        self.hw_probe_runners();
    }

    /// `runners --json` on this core — the dialog's one probe; the
    /// reply lands on `ns_rx` like the wizard's.
    fn hw_probe_runners(&mut self) {
        let run = self.ns_runner();
        let tx = self.ns_tx.clone();
        let Some(d) = &mut self.hw_setup else {
            return;
        };
        d.runners_gen += 1;
        let epoch = d.runners_gen;
        d.runners = Probe::Pending;
        tokio::spawn(async move {
            let out = run(None, vec!["runners".into(), "--json".into()], None).await;
            let _ = tx.send(NsReply::HwRunners { epoch, out }).await;
        });
    }

    /// The `HwRunners` reply — land it only while the dialog and
    /// generation still match, and seat the existing table's values
    /// on the first answer.
    pub(crate) fn hw_runners_reply(&mut self, epoch: u64, out: Result<String, String>) {
        let Some(d) = &mut self.hw_setup else {
            return;
        };
        if epoch != d.runners_gen {
            return;
        }
        d.runners = match out {
            Ok(text) => match new_studio::parse_runners(&text) {
                Ok(rows) => Probe::Done(rows),
                Err(e) => Probe::Failed(e),
            },
            Err(e) => Probe::Failed(e),
        };
        // The table's values seat on the first answer — never on a
        // still-pending or failed read.
        d.prefill();
    }

    /// Keys while the dialog is open: the modal chain's arm. An open
    /// pick list or free-text editor takes its own keys first —
    /// `Esc` closes the innermost, then the dialog; `^C` cancels
    /// from anywhere like the wizard's.
    pub(super) fn hw_setup_key(&mut self, key: &TerminalKey) {
        if key.input.key == Key::Char('c') && key.input.mods == Mods::CTRL {
            self.hw_setup = None;
            return;
        }
        let Some(d) = &mut self.hw_setup else {
            return;
        };
        if d.model_pick.is_some() {
            match key.input.key {
                Key::Escape => d.model_pick = None,
                Key::Enter => {
                    let model = d
                        .model_pick
                        .as_ref()
                        .and_then(|pick| pick.chosen().map(str::to_string));
                    d.model_pick = None;
                    if let Some(model) = model {
                        d.model = model;
                    }
                }
                Key::ArrowUp | Key::ArrowDown => {
                    let pick = d.model_pick.as_mut().unwrap();
                    let n = pick.visible().len();
                    if n > 0 {
                        let delta = if key.input.key == Key::ArrowUp { -1 } else { 1 };
                        pick.sel = (pick.sel as isize + delta).rem_euclid(n as isize) as usize;
                    }
                }
                _ => {
                    let pick = d.model_pick.as_mut().unwrap();
                    if pick.filter.handle_key(key) == Some(true) {
                        pick.sel = 0;
                        pick.scroll = 0;
                    }
                }
            }
            return;
        }
        if d.model_edit.is_some() {
            match key.input.key {
                Key::Enter => {
                    let Some(edit) = d.model_edit.take() else {
                        return;
                    };
                    let text = edit.as_str().trim().to_string();
                    if !text.is_empty() {
                        d.model = text;
                    }
                }
                Key::Escape => d.model_edit = None,
                _ => {
                    if let Some(edit) = &mut d.model_edit {
                        edit.handle_key(key);
                    }
                }
            }
            return;
        }
        match key.input.key {
            Key::Escape => self.hw_setup = None,
            Key::Enter => match d.field {
                Field::Model => d.open_pick(),
                Field::Apply => self.hw_setup_apply(),
                _ => d.cycle(1),
            },
            Key::ArrowUp => d.move_field(-1),
            Key::ArrowDown => d.move_field(1),
            Key::Tab => d.move_field(1),
            Key::BackTab => d.move_field(-1),
            Key::ArrowLeft => d.cycle(-1),
            Key::ArrowRight => d.cycle(1),
            Key::Char(' ') if d.field == Field::Model => d.open_pick(),
            Key::Char(' ') if d.field == Field::Apply => self.hw_setup_apply(),
            Key::Char('e') if d.field == Field::Model && key.input.mods == Mods::NONE => {
                d.model_edit = Some(crate::chrome::text_editor::TextEditor::new(&d.model, false));
            }
            Key::Char('r') if key.input.mods == Mods::CTRL => self.hw_probe_runners(),
            _ => {}
        }
    }

    /// `⏎`/space on `apply` — `backlot hollywood setup` through the
    /// action tracker (the toast is its own); the table's untouched
    /// `key`/`default_cast` ride along verbatim on a reconfigure,
    /// and the cfg is remembered so the write's `Ok` flips the bar.
    fn hw_setup_apply(&mut self) {
        let Some(d) = &self.hw_setup else { return };
        let Some(cfg) = d.cfg() else { return };
        self.hw_setup = None;
        self.hw_setup_pending = Some(cfg.clone());
        self.run_action(crate::studio_cmds::hw_setup(&cfg));
    }

    /// Mouse while the dialog is open: a click outside closes (the
    /// overlay's rule), a cell selects what it paints, the wheel
    /// walks an open list.
    pub(super) fn hw_setup_mouse(&mut self, mouse: MouseInput) {
        if matches!(
            mouse.kind,
            MouseEventKind::ScrollUp | MouseEventKind::ScrollDown
        ) {
            let delta = if matches!(mouse.kind, MouseEventKind::ScrollUp) {
                -1
            } else {
                1
            };
            if let Some(d) = self.hw_setup.as_mut()
                && let Some(pick) = d.model_pick.as_mut()
            {
                let n = pick.visible().len();
                if n > 0 {
                    pick.sel = (pick.sel as isize + delta).rem_euclid(n as isize) as usize;
                }
            }
            return;
        }
        if mouse.kind != MouseEventKind::Down(MouseButton::Left) {
            return;
        }
        if !contains(self.hw_hits.area, (mouse.col, mouse.row)) {
            self.hw_setup = None;
            return;
        }
        let hit = self
            .hw_hits
            .cells
            .iter()
            .find(|(rect, _)| contains(*rect, (mouse.col, mouse.row)))
            .map(|(_, hit)| hit.clone());
        let Some(hit) = hit else { return };
        match hit {
            HwHit::ModelPick(index) => {
                if let Some(d) = self.hw_setup.as_mut()
                    && let Some(pick) = d.model_pick.as_mut()
                {
                    pick.sel = index;
                    let model = pick.chosen().map(str::to_string);
                    d.model_pick = None;
                    if let Some(model) = model {
                        d.model = model;
                    }
                }
            }
            HwHit::Field(field) => {
                if let Some(d) = self.hw_setup.as_mut() {
                    if d.field == field {
                        // A second press on the focused row is its Enter.
                        match field {
                            Field::Model => d.open_pick(),
                            Field::Apply => self.hw_setup_apply(),
                            _ => d.cycle(1),
                        }
                    } else {
                        d.field = field;
                        if field == Field::Model {
                            d.open_pick();
                        }
                    }
                }
            }
        }
    }

    /// IME/commit text and pastes while the dialog is open land in
    /// the live field — the model list's always-live filter or the
    /// `e` free-text editor — never in the terminal behind it.
    pub(super) fn insert_hw_text(&mut self, text: &str) -> bool {
        let Some(d) = &mut self.hw_setup else {
            return false;
        };
        if let Some(pick) = &mut d.model_pick {
            if pick.filter.insert(text) {
                pick.sel = 0;
                pick.scroll = 0;
            }
            return true;
        }
        if let Some(edit) = &mut d.model_edit {
            edit.insert(text);
            return true;
        }
        true
    }

    /// The setup write's reply — `Ok` flips the layer to its agent
    /// form exactly as `set_hollywood` does at start; a failure only
    /// clears the record — the refusal is the toast's own text.
    pub(crate) fn hw_setup_reply(&mut self, ok: bool) {
        let Some(cfg) = self.hw_setup_pending.take() else {
            return;
        };
        if ok {
            self.has_hollywood_cfg = true;
            self.hollywood_overseer = Some(format!("{} · {}", cfg.runner, cfg.model));
            self.panel.hollywood_hint = false;
            self.hollywood_cfg = Some(cfg);
        }
    }
}
