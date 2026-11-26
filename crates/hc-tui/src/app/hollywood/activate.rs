//! The expanded sheet's activation (`Enter` / left-click on a row
//! or focused action chip): running the focused chip on waiting rows,
//! focusing `bl-hollywood` or a role's terminal, starting a stopped
//! stage, opening a card/headline/talk detail view, or opening the
//! input/setup/new-studio dialog.

use crossterm::event::KeyCode;

use super::App;
use crate::hollywood::{self, Target};
use crate::studio::StudioKey;

impl App {
    /// `Enter` in `Resting` mode: on a `Wait` row, run the currently
    /// highlighted action chip (`allow y` on prompts, `show ↵` on cards,
    /// `start s` on stages, `finish f` on ready studios, etc.); on any
    /// other row, run its default activation.
    pub(super) fn hollywood_enter(&mut self) {
        let targets = self.hollywood_targets();
        let index = self.hollywood_selected_index(&targets);
        let Some(selected) = targets.get(index).cloned() else {
            return;
        };
        let key_to_run = {
            let machines = self.panel_summaries();
            let status = hollywood::status_of(&self.view);
            let waits = hollywood::waiting_items(&machines, &status);
            waits
                .iter()
                .find(|w| w.target() == selected)
                .and_then(|wait| {
                    let chips = hollywood::chips_of(wait);
                    if chips.is_empty() {
                        return None;
                    }
                    let idx = self
                        .hollywood
                        .chip_index
                        .unwrap_or_else(|| hollywood::default_chip_index(wait, &chips))
                        .min(chips.len() - 1);
                    Some(chips[idx].key)
                })
        };
        match key_to_run {
            Some("↵") | None => self.hollywood_activate(),
            Some(key) => {
                if let Some(ch) = key.chars().next() {
                    self.hollywood_row_key(KeyCode::Char(ch));
                }
            }
        }
    }

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
            Some(Target::Prompt(studio, role)) | Some(Target::Role(studio, role)) => {
                self.hollywood_prompt_open(&studio, &role);
            }
            Some(Target::Hollywood | Target::HollywoodLive) => self.hollywood_focus_terminal(),
            Some(Target::Tell(key) | Target::Upgrade(key) | Target::Decision(key)) => {
                self.focus_studio(&key);
                self.hollywood_leave();
            }
            Some(Target::Stage(machine, offline)) => {
                if offline {
                    self.hollywood_check_machine(machine.as_deref());
                } else {
                    self.hollywood_stage_start(machine);
                }
            }
            Some(Target::Card(id)) => self.hollywood_card_show_on(None, &id),
            Some(Target::OutwardCard(key, id)) => {
                self.hollywood_card_show_on(key.machine, &id);
            }
            Some(Target::Ready(key)) => {
                self.focus_studio(&key);
                self.hollywood_leave();
            }
            Some(Target::Input) => {
                self.hollywood_select(Target::Input);
                self.hollywood.editing = true;
            }
            Some(Target::Headline) => self.hollywood_headline_open(),
            Some(Target::Talk(seq)) => {
                let card = self.hollywood_exchange_is_card(seq);
                self.run_action(crate::studio_cmds::hollywood_exchange(seq, card));
            }
            // U7's setup line: the in-TUI dialog — runner/model/
            // effort pickers, the write through `hollywood setup`.
            Some(Target::Setup) => self.hw_switch_open(false),
            _ => {}
        }
    }

    /// Whether the exchange entry at `seq` is Hollywood's own card
    /// line (reads `card show`), not a report or message.
    pub(crate) fn hollywood_exchange_is_card(&self, seq: u64) -> bool {
        let machines = self.panel_summaries();
        hollywood::local_summary(&machines)
            .and_then(|s| s.hollywood.as_ref())
            .and_then(|h| h.exchange.iter().find(|e| e.seq == seq))
            .is_some_and(|e| e.kind.as_deref() == Some("card"))
    }

    /// Resolve the ledger `seq` backing `Target::Headline` — prefers
    /// `headline_seq` and falls back to the newest Hollywood line in
    /// `exchange` when reading an older stage's document.
    pub(crate) fn hollywood_headline_seq(&self) -> Option<u64> {
        let machines = self.panel_summaries();
        let hw = hollywood::local_summary(&machines)?.hollywood.as_ref()?;
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
        if let Some(headline) = hollywood::local_summary(&machines)
            .and_then(|s| s.hollywood.as_ref())
            .and_then(|h| h.headline.as_deref())
        {
            let lines: Vec<String> = headline.lines().map(str::to_string).collect();
            if !lines.is_empty() {
                let meta = self.reader_meta_for_headline(&machines);
                self.hollywood.view = Some(hollywood::CardView {
                    card: None,
                    title: "hollywood".into(),
                    lines,
                    raw_body: headline.to_owned(),
                    scroll: 0,
                    meta,
                    ..hollywood::CardView::default()
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

    /// The live active Hollywood terminal's address: prefers `summary.hollywood.terminal`,
    /// falls back to the unique live `backlot.role == "hollywood"` terminal, then to the
    /// legacy `HOLLYWOOD_TERMINAL` (`bl-hollywood`) name.
    pub(crate) fn hollywood_terminal(&self) -> Option<String> {
        let machines = self.panel_summaries();
        let summary_addr = crate::hollywood::local_summary(&machines)
            .and_then(|s| s.hollywood.as_ref())
            .and_then(|h| h.terminal.as_deref());
        let terminals = self.view.inventory.terminals();
        if let Some(addr) = summary_addr
            && terminals
                .values()
                .any(|t| t.state == "live" && !t.address.contains('/') && t.address == addr)
        {
            return Some(addr.to_string());
        }
        terminals
            .values()
            .find(|t| {
                t.state == "live"
                    && !t.address.contains('/')
                    && t.labels.get("backlot.role").map(String::as_str) == Some("hollywood")
            })
            .or_else(|| {
                terminals.values().find(|t| {
                    t.state == "live"
                        && !t.address.contains('/')
                        && t.name
                            .as_deref()
                            .is_some_and(backlot_model::hollywood::is_hollywood_terminal_name)
                })
            })
            .map(|t| t.address.clone())
    }

    /// Live candidate Hollywood terminal during an in-progress handoff.
    pub(crate) fn candidate_hollywood_terminal(&self) -> Option<String> {
        let machines = self.panel_summaries();
        let cand_addr = crate::hollywood::local_summary(&machines)
            .and_then(|s| s.hollywood.as_ref())
            .and_then(|h| h.handoff.as_ref())
            .and_then(|hf| hf.candidate_terminal.as_deref());
        let terminals = self.view.inventory.terminals();
        if let Some(addr) = cand_addr
            && terminals
                .values()
                .any(|t| t.state == "live" && !t.address.contains('/') && t.address == addr)
        {
            return Some(addr.to_string());
        }
        terminals
            .values()
            .find(|t| {
                t.state == "live"
                    && !t.address.contains('/')
                    && t.labels.get("backlot.role").map(String::as_str) == Some("hollywood-handoff")
            })
            .map(|t| t.address.clone())
    }

    /// Focus the `bl-hollywood` terminal.
    pub(crate) fn hollywood_focus_terminal(&mut self) {
        match self.hollywood_terminal() {
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
