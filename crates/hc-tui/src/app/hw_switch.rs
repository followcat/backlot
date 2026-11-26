//! App wiring for the unified Hollywood switch / setup picker (§2.2).

use hc_vt::{Key, Mods};

use super::new_studio::NsReply;
use super::{App, input::TerminalKey};
use crate::app::input::{MouseButton, MouseEventKind, MouseInput};
use crate::app::mouse::contains;
use crate::hw_switch::{Field, HwHit, HwSwitch};
use crate::new_studio::{self, Probe};

impl App {
    /// Open the unified Hollywood switch/setup modal.
    /// When `prefer_alternative` is `true` (e.g. from an out-of-quota row)
    /// and multiple runners are installed, highlights another runner.
    pub(crate) fn hw_switch_open(&mut self, prefer_alternative: bool) {
        if self.plain {
            return;
        }
        self.hw_switch = Some(HwSwitch::new(
            self.hollywood_cfg.clone(),
            prefer_alternative,
        ));
        self.hw_probe_runners();
    }

    /// Keep `hw_setup_open` for internal/test ergonomics forwarding to `hw_switch_open(false)`.
    #[cfg(test)]
    pub(crate) fn hw_setup_open(&mut self) {
        self.hw_switch_open(false);
    }

    fn hw_probe_runners(&mut self) {
        let run = self.ns_runner();
        let tx = self.ns_tx.clone();
        let Some(d) = &mut self.hw_switch else {
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

    pub(crate) fn hw_runners_reply(&mut self, epoch: u64, out: Result<String, String>) {
        let Some(d) = &mut self.hw_switch else {
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
        d.prefill();
    }

    pub(super) fn hw_switch_key(&mut self, key: &TerminalKey) {
        if key.input.key == Key::Char('c') && key.input.mods == Mods::CTRL {
            self.hw_switch = None;
            return;
        }
        let Some(d) = &mut self.hw_switch else {
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
                        d.error = None;
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
                        d.error = None;
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
            Key::Escape => self.hw_switch = None,
            Key::Enter => match d.field {
                Field::Model => d.open_pick(),
                Field::Apply => self.hw_switch_apply(),
                _ => d.cycle(1),
            },
            Key::ArrowUp => d.move_field(-1),
            Key::ArrowDown => d.move_field(1),
            Key::Tab => d.move_field(1),
            Key::BackTab => d.move_field(-1),
            Key::ArrowLeft => d.cycle(-1),
            Key::ArrowRight => d.cycle(1),
            Key::Char(' ') if d.field == Field::Model => d.open_pick(),
            Key::Char(' ') if d.field == Field::Apply => self.hw_switch_apply(),
            Key::Char('e') if d.field == Field::Model && key.input.mods == Mods::NONE => {
                d.error = None;
                d.model_edit = Some(crate::chrome::text_editor::TextEditor::new(&d.model, false));
            }
            Key::Char('r') if key.input.mods == Mods::CTRL => self.hw_probe_runners(),
            _ => {}
        }
    }

    pub(crate) fn hollywood_is_live_switchable(&self) -> bool {
        if !self.has_hollywood_cfg {
            return false;
        }
        let machines = self.panel_summaries();
        let hw_live = crate::hollywood::local_summary(&machines)
            .and_then(|s| s.hollywood.as_ref())
            .is_some_and(|h| h.state == "live");
        self.machine_stage_running(None) || hw_live
    }

    fn hw_switch_apply(&mut self) {
        let live_switch = self.hollywood_is_live_switchable();
        let Some(d) = &mut self.hw_switch else {
            return;
        };
        if d.unchanged() {
            d.error = Some("identity unchanged — pick a different runner, model, or effort".into());
            return;
        }
        let Some(cfg) = d.cfg() else {
            d.error = Some("pick a runner and non-empty model".into());
            return;
        };
        self.hw_switch = None;
        if live_switch {
            self.run_action(crate::studio_cmds::hw_switch(
                &cfg.runner,
                &cfg.model,
                cfg.effort.as_deref(),
            ));
        } else {
            self.hw_setup_pending = Some(cfg.clone());
            self.run_action(crate::studio_cmds::hw_setup(&cfg));
        }
    }

    pub(super) fn hw_switch_mouse(&mut self, mouse: MouseInput) {
        if matches!(
            mouse.kind,
            MouseEventKind::ScrollUp | MouseEventKind::ScrollDown
        ) {
            let delta = if matches!(mouse.kind, MouseEventKind::ScrollUp) {
                -1
            } else {
                1
            };
            if let Some(d) = self.hw_switch.as_mut()
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
            self.hw_switch = None;
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
                if let Some(d) = self.hw_switch.as_mut()
                    && let Some(pick) = d.model_pick.as_mut()
                {
                    pick.sel = index;
                    let model = pick.chosen().map(str::to_string);
                    d.model_pick = None;
                    if let Some(model) = model {
                        d.model = model;
                        d.error = None;
                    }
                }
            }
            HwHit::Field(field) => {
                if let Some(d) = self.hw_switch.as_mut() {
                    if d.field == field {
                        match field {
                            Field::Model => d.open_pick(),
                            Field::Apply => self.hw_switch_apply(),
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

    pub(super) fn insert_hw_text(&mut self, text: &str) -> bool {
        let Some(d) = &mut self.hw_switch else {
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

    pub(crate) fn hw_setup_reply(&mut self, ok: bool) {
        let Some(cfg) = self.hw_setup_pending.take() else {
            return;
        };
        if ok {
            self.has_hollywood_cfg = true;
            self.hollywood_overseer = Some(super::hollywood::overseer_label(&cfg));
            self.panel.hollywood_hint = false;
            self.hollywood_cfg = Some(cfg);
        }
    }
}
