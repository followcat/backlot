//! The expanded sheet's cursor: the frame walk, the band walk, the
//! scroll windows that keep the selected row drawn, and Enter's
//! activation — what each row's "open" means. Selection is always
//! by stable `Target`, never by position.

use super::App;
use crate::hollywood::{LayerMode, Target};
use crate::studio::StudioKey;

impl App {
    // ---------- frames and rows ----------

    /// ← / → (`h` / `l`): move the frame cursor, the new-studio frame
    /// included, and keep it in the drawn window.
    pub(crate) fn hollywood_frame_move(&mut self, delta: isize) {
        let frames = self.hollywood_frames();
        if frames.is_empty() {
            return;
        }
        let current = self
            .hollywood
            .selected
            .as_ref()
            .and_then(|t| frames.iter().position(|f| f == t))
            .unwrap_or(0);
        let next = (current as isize + delta).clamp(0, frames.len() as isize - 1) as usize;
        self.hollywood.selected = Some(frames[next].clone());
        self.hollywood_keep_frame_visible();
    }

    /// ↑ / ↓ (`k` / `j`): from the frames into the band's rows, then
    /// along them — and back to the frames from the band's first row.
    /// On a narrow screen the frames move a row at a time.
    pub(crate) fn hollywood_row_move(&mut self, delta: isize) {
        let frames = self.hollywood_frames();
        let band = self.hollywood_band();
        if let Some(index) = self
            .hollywood
            .selected
            .as_ref()
            .and_then(|t| band.iter().position(|r| r == t))
        {
            if index == 0 && delta < 0 {
                // Back up into the frames.
                if let Some(last) = frames.last() {
                    self.hollywood.selected = Some(last.clone());
                    self.hollywood_keep_frame_visible();
                }
                return;
            }
            let next = (index as isize + delta).clamp(0, band.len() as isize - 1) as usize;
            self.hollywood.selected = Some(band[next].clone());
            self.hollywood_reveal_card(&band, next);
            self.hollywood_sync_editing();
            return;
        }
        let current = self
            .hollywood
            .selected
            .as_ref()
            .and_then(|t| frames.iter().position(|f| f == t))
            .unwrap_or(0);
        let mobile = self.area.0 <= self.shell.mobile_width_threshold;
        let step = if mobile {
            self.hollywood_frame_window().1.max(1) as isize
        } else {
            frames.len().max(1) as isize
        };
        let next = current as isize + delta.signum() * step;
        if next < 0 {
            return;
        }
        if (next as usize) < frames.len() {
            self.hollywood.selected = Some(frames[next as usize].clone());
            self.hollywood_keep_frame_visible();
        } else if let Some(first) = band.first() {
            self.hollywood.selected = Some(first.clone());
            self.hollywood_reveal_card(&band, 0);
        }
        self.hollywood_sync_editing();
    }

    /// Scroll the frame window so the selected frame shows, whole rows
    /// at a time so a narrow screen never starts mid-row.
    pub(super) fn hollywood_keep_frame_visible(&mut self) {
        let Some(index) = self
            .hollywood
            .selected
            .as_ref()
            .and_then(|t| self.hollywood_frames().iter().position(|f| f == t))
        else {
            return;
        };
        let (visible, per_row) = self.hollywood_frame_window();
        if visible == 0 {
            return;
        }
        let rows = (visible / per_row).max(1);
        let mut start = self.hollywood.frame_scroll;
        let row = index / per_row;
        let start_row = start / per_row;
        if row < start_row {
            start = row * per_row;
        } else if row >= start_row + rows {
            start = (row + 1 - rows) * per_row;
        }
        self.hollywood.frame_scroll = start;
    }

    /// Keep the selected waiting row inside the drawn window.
    pub(super) fn hollywood_reveal_card(&mut self, band: &[Target], index: usize) {
        let waits = |slice: &[Target]| {
            slice
                .iter()
                .filter(|t| {
                    matches!(
                        t,
                        Target::Prompt(..) | Target::Stage(..) | Target::Card(_) | Target::Ready(_)
                    )
                })
                .count()
        };
        if !matches!(
            band.get(index),
            Some(Target::Prompt(..) | Target::Stage(..) | Target::Card(_) | Target::Ready(_))
        ) {
            return;
        }
        let wait = waits(&band[..index]);
        let visible = self.hollywood_hits.viewport_rows.max(1);
        if wait < self.hollywood.scroll {
            self.hollywood.scroll = wait;
        } else if wait >= self.hollywood.scroll + visible {
            self.hollywood.scroll = wait + 1 - visible;
        }
    }

    /// Wheel/drag scroll of the cards: clamp the offset into the drawn
    /// metrics.
    pub(crate) fn hollywood_scroll_to(&mut self, start: usize) {
        let max = self
            .hollywood_hits
            .metrics
            .map(|m| m.max_offset_from_bottom)
            .unwrap_or(0);
        self.hollywood.scroll = start.min(max);
    }

    /// The frames' rect as last drawn — the wheel's sideways target.
    pub(crate) fn hollywood_frames_hit(&self) -> ratatui::layout::Rect {
        self.hollywood_hits.body
    }

    /// Leave the layer after a row focused a terminal: the panel
    /// collapses back to its strip so the focused pane stays visible.
    pub(super) fn hollywood_leave(&mut self) {
        self.hollywood.mode = LayerMode::Collapsed;
        self.hollywood.editing = false;
        self.hollywood.view = None;
    }

    // ---------- activation ----------

    /// Enter/left-click on a row: the title focuses `bl-hollywood`,
    /// a studio row focuses that studio through the panel's own path,
    /// a prompt or ready row focuses its terminal, a stage-down row
    /// restarts the stage, a card row is its detail view, the input
    /// line opens for editing.
    pub(super) fn hollywood_activate(&mut self) {
        let targets = self.hollywood_targets();
        let index = self.hollywood_selected_index(&targets);
        match targets.get(index).cloned() {
            Some(Target::Title) => self.hollywood_focus_terminal(),
            Some(Target::Studio(id)) => {
                self.focus_studio(&id);
                self.hollywood_leave();
            }
            Some(Target::NewStudio) => self.hollywood_new_studio(),
            Some(Target::Prompt(studio, role)) => self.hollywood_prompt_open(&studio, &role),
            Some(Target::Stage(machine, offline)) => {
                if offline {
                    self.hollywood_check_machine(machine.as_deref());
                } else {
                    self.hollywood_stage_start(machine);
                }
            }
            Some(Target::Card(id)) => self.hollywood_card_show(&id),
            Some(Target::Ready(key)) => {
                self.focus_studio(&key);
                self.hollywood_leave();
            }
            Some(Target::Input) => {
                self.hollywood.selected = Some(Target::Input);
                self.hollywood.editing = true;
            }
            Some(Target::Headline) => self.hollywood_headline_open(),
            Some(Target::Talk(seq)) => {
                self.run_action(crate::studio_cmds::hollywood_show(seq));
            }
            // U7's setup line: the in-TUI dialog — runner/model/
            // effort pickers, the write through `hollywood setup`.
            Some(Target::Setup) => self.hw_setup_open(),
            _ => {}
        }
    }

    /// Resolve the ledger `seq` backing `Target::Headline` — prefers
    /// `headline_seq` and falls back to the newest Hollywood line in
    /// `exchange` when reading an older stage's document.
    pub(crate) fn hollywood_headline_seq(&self) -> Option<u64> {
        let machines = self.panel_summaries();
        let hw = crate::hollywood::local_summary(&machines)?
            .hollywood
            .as_ref()?;
        hw.headline_seq.or_else(|| {
            hw.exchange
                .iter()
                .rfind(|e| e.who == "hollywood")
                .map(|e| e.seq)
        })
    }

    /// Open `Target::Headline` in the read-only `CardView`: fetches
    /// `backlot hollywood show <seq>` when a seq is known, or shows
    /// the published headline text directly on legacy documents.
    pub(crate) fn hollywood_headline_open(&mut self) {
        if let Some(seq) = self.hollywood_headline_seq() {
            self.run_action(crate::studio_cmds::hollywood_show(seq));
            return;
        }
        let machines = self.panel_summaries();
        if let Some(headline) = crate::hollywood::local_summary(&machines)
            .and_then(|s| s.hollywood.as_ref())
            .and_then(|h| h.headline.as_deref())
        {
            let lines: Vec<String> = headline.lines().map(str::to_string).collect();
            if !lines.is_empty() {
                self.hollywood.view = Some(crate::hollywood::CardView {
                    card: None,
                    title: "hollywood".into(),
                    lines,
                    scroll: 0,
                });
            }
        }
    }

    /// A prompt's `open ↵`: focus the role's terminal — already
    /// machine-qualified for a remote studio — and collapse.
    pub(super) fn hollywood_prompt_open(&mut self, studio: &StudioKey, role: &str) {
        let machines = self.panel_summaries();
        let terminal = crate::panel::data::studio(&machines, studio).and_then(|s| {
            s.studio
                .roles
                .iter()
                .find(|r| r.role == role)
                .and_then(|r| r.terminal.clone())
        });
        match terminal {
            Some(terminal) => {
                self.focus_pane(terminal);
                self.hollywood_leave();
            }
            None => self.show_notice("Hollywood", "the role has no terminal to focus"),
        }
    }

    /// An offline machine's `check machine ↵`: the watcher's last
    /// read of the line, where the user is looking.
    pub(super) fn hollywood_check_machine(&mut self, machine: Option<&str>) {
        let name = machine.unwrap_or(crate::project::LOCAL_LABEL);
        let why = self
            .view
            .machines
            .iter()
            .flatten()
            .find(|m| Some(m.name.as_str()) == machine)
            .map(|m| {
                let state = m.state.as_str();
                match m.why.as_deref() {
                    Some(why) => format!("{state} — {why}"),
                    None => state.to_string(),
                }
            })
            .unwrap_or_else(|| "unreachable".to_string());
        self.show_notice("Machine", &format!("{name}: {why}"));
    }

    /// The new-studio frame, and `n` anywhere in the bar: the bar
    /// collapses first so the wizard's popup is never under it, then
    /// the wizard runs — the same one the opening card's `n` opens.
    pub(super) fn hollywood_new_studio(&mut self) {
        self.hollywood_leave();
        self.opening_new_studio();
    }

    /// Focus the `bl-hollywood` terminal — same read `reap` and
    /// `stage_running` make of the inventory: the terminal named so.
    pub(crate) fn hollywood_focus_terminal(&mut self) {
        let terminal = self
            .view
            .inventory
            .terminals()
            .values()
            .find(|t| {
                t.state == "live"
                    && !t.address.contains('/')
                    && t.name.as_deref() == Some(backlot_model::hollywood::HOLLYWOOD_TERMINAL)
            })
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
}
