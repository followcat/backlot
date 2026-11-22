//! The Task & Delivery Sheet's App wiring (S2 §3): `v` opens it on
//! the dock's selected studio/task, `status --json` and
//! `records --json` fetch asynchronously on the studio's machine —
//! the New Studio runner shared — and `o` streams a delivery's
//! report or a check's log through `backlot report show` (G3).
//! Replies carry the open epoch so a sheet already closed or
//! reopened drops a late answer.

use super::App;
use crate::app::input::TerminalKey;
use crate::panel::Target;
use crate::sheet::{Sheet, SheetTab, lines_for};
use crate::studio::StudioKey;

/// One async answer the sheet waits on, tagged by open epoch —
/// `view_epoch` tags a report read inside one open.
pub(crate) enum SheetReply {
    Status {
        epoch: u64,
        out: Result<String, String>,
    },
    Records {
        epoch: u64,
        out: Result<String, String>,
    },
    Report {
        epoch: u64,
        view_epoch: u64,
        title: String,
        out: Result<String, String>,
    },
}

impl App {
    /// `v` on the dock: the sheet for the selected studio — the
    /// selected task row's task, a person's own task, or none —
    /// and the two ledger reads it fills in behind the summary.
    pub(super) fn panel_sheet_open(&mut self) {
        let targets = self.panel_targets();
        let index = self.panel_selected_index(&targets);
        let (key, task) = match targets.get(index) {
            Some(Target::Task(key, id)) => (key.clone(), Some(id.clone())),
            Some(Target::Person(key, role)) => {
                let machines = self.panel_summaries();
                let task = crate::panel::data::studio(&machines, key).and_then(|s| {
                    s.studio
                        .roles
                        .iter()
                        .find(|r| r.role == *role)
                        .and_then(|r| r.task.clone())
                });
                (key.clone(), task)
            }
            Some(target) => match target.studio() {
                Some(key) => (key.clone(), None),
                None => return,
            },
            None => return,
        };
        self.sheet_open(key, task);
    }

    /// Open the sheet for `key`/`task` and start the fetches.
    /// Opening again bumps the epoch so the earlier sheet's late
    /// replies land dead (S2 §3).
    pub(crate) fn sheet_open(&mut self, key: StudioKey, task: Option<String>) {
        self.sheet_epoch += 1;
        let epoch = self.sheet_epoch;
        self.sheet = Some(Sheet::new(key.clone(), task, epoch));
        self.sheet_fetch(epoch, &key);
    }

    /// `backlot status --json` and `backlot records --json` on the
    /// studio's machine — local through `current_exe`, remote
    /// through `RemoteRunner::run_output` — both tagged by epoch.
    fn sheet_fetch(&mut self, epoch: u64, key: &StudioKey) {
        let run = self.ns_runner();
        let machine = key.machine.clone();
        for (argv, kind) in [
            (
                vec![
                    "status".into(),
                    "--studio".into(),
                    key.id.clone(),
                    "--json".into(),
                ],
                0u8,
            ),
            (
                vec![
                    "records".into(),
                    "--studio".into(),
                    key.id.clone(),
                    "--json".into(),
                ],
                1u8,
            ),
        ] {
            let run = run.clone();
            let machine = machine.clone();
            let tx = self.sheet_tx.clone();
            tokio::spawn(async move {
                let out = run(machine, argv, None).await;
                let reply = match kind {
                    0 => SheetReply::Status { epoch, out },
                    _ => SheetReply::Records { epoch, out },
                };
                let _ = tx.send(reply).await;
            });
        }
    }

    /// `o` inside the sheet: the selected line's file streams
    /// through `backlot report show` — a local studio's run reads
    /// its own file; a remote studio's runs the same G3 command on
    /// its machine (S2 §3). On the Deliveries tab only.
    pub(crate) fn sheet_open_file(&mut self) {
        let Some(sheet) = self.sheet.as_ref() else {
            return;
        };
        if sheet.view.is_some() || sheet.tab != SheetTab::Deliveries {
            return;
        }
        let summary = self
            .panel_summaries()
            .iter()
            .find_map(|m| m.summary.studios.iter().find(|s| s.id == sheet.studio.id))
            .cloned();
        let file = lines_for(sheet, summary.as_ref())
            .get(sheet.sel)
            .and_then(|l| l.file.clone());
        let Some(file) = file else {
            return;
        };
        let run = self.ns_runner();
        let machine = sheet.studio.machine.clone();
        let argv = vec![
            "report".into(),
            "show".into(),
            "--studio".into(),
            sheet.studio.id.clone(),
            "--file".into(),
            file.clone(),
        ];
        let epoch = sheet.epoch;
        let sheet_mut = self.sheet.as_mut().expect("checked above");
        sheet_mut.view_epoch += 1;
        let view_epoch = sheet_mut.view_epoch;
        let tx = self.sheet_tx.clone();
        tokio::spawn(async move {
            let out = run(machine, argv, None).await;
            let _ = tx
                .send(SheetReply::Report {
                    epoch,
                    view_epoch,
                    title: file,
                    out,
                })
                .await;
        });
    }

    /// A reply off `sheet_rx` — landed only while the sheet it was
    /// fetched for is still open (a second open bumped `epoch`).
    pub(crate) fn on_sheet_reply(&mut self, reply: SheetReply) {
        let Some(sheet) = self.sheet.as_mut() else {
            return;
        };
        match reply {
            SheetReply::Status { epoch, out } => {
                if epoch != sheet.epoch {
                    return;
                }
                sheet.status = Some(out.and_then(|text| {
                    serde_json::from_str(&text).map_err(|e| format!("bad status json: {e}"))
                }));
            }
            SheetReply::Records { epoch, out } => {
                if epoch != sheet.epoch {
                    return;
                }
                sheet.records = Some(out.and_then(|text| {
                    serde_json::from_str(&text).map_err(|e| format!("bad records json: {e}"))
                }));
            }
            SheetReply::Report {
                epoch,
                view_epoch,
                title,
                out,
            } => {
                if epoch != sheet.epoch || view_epoch != sheet.view_epoch {
                    return;
                }
                match out {
                    Ok(text) => {
                        sheet.view = Some(crate::sheet::SheetView {
                            title,
                            lines: text.lines().map(str::to_string).collect(),
                            scroll: 0,
                        });
                    }
                    Err(err) => {
                        self.show_notice("Report", &err);
                    }
                }
            }
        }
        self.sched.mark_dirty(std::time::Instant::now());
    }

    /// Keys while the sheet is open — it owns every press like the
    /// other modals; `Esc` backs out of an opened report first.
    pub(crate) fn handle_sheet_key(&mut self, key: &TerminalKey) {
        use crossterm::event::KeyCode;
        if key.input.action != hc_vt::Action::Press {
            return;
        }
        let combo = crate::input::terminal_key_of(key);
        let Some(sheet) = self.sheet.as_mut() else {
            return;
        };
        // An opened report eats movement as its own scroll; `Esc`
        // or `o` retires it back to the tab body.
        if sheet.view.is_some() {
            match combo.code {
                KeyCode::Esc | KeyCode::Char('o') | KeyCode::Char('q') => {
                    sheet.view = None;
                }
                KeyCode::Down | KeyCode::Char('j') => {
                    if let Some(view) = sheet.view.as_mut() {
                        view.scroll += 1;
                    }
                }
                KeyCode::Up | KeyCode::Char('k') => {
                    if let Some(view) = sheet.view.as_mut() {
                        view.scroll = view.scroll.saturating_sub(1);
                    }
                }
                KeyCode::PageDown => {
                    if let Some(view) = sheet.view.as_mut() {
                        view.scroll += 10;
                    }
                }
                KeyCode::PageUp => {
                    if let Some(view) = sheet.view.as_mut() {
                        view.scroll = view.scroll.saturating_sub(10);
                    }
                }
                _ => {}
            }
            return;
        }
        match combo.code {
            KeyCode::Esc | KeyCode::Char('q') => {
                self.sheet = None;
            }
            KeyCode::Tab | KeyCode::BackTab => {
                sheet.tab = sheet.tab.next();
                sheet.sel = 0;
                sheet.scroll = 0;
            }
            KeyCode::Char(c @ '1'..='3') => {
                if let Some(tab) = SheetTab::of(c) {
                    sheet.tab = tab;
                    sheet.sel = 0;
                    sheet.scroll = 0;
                }
            }
            KeyCode::Down | KeyCode::Char('j') => self.sheet_move(1),
            KeyCode::Up | KeyCode::Char('k') => self.sheet_move(-1),
            KeyCode::PageDown => self.sheet_page(1),
            KeyCode::PageUp => self.sheet_page(-1),
            KeyCode::Char('o') => self.sheet_open_file(),
            _ => {}
        }
    }

    /// Sheet mouse: a press outside the frame cancels like every
    /// modal; the wheel scrolls the open report or the tab body; a
    /// press on a selectable body line lands the selection there —
    /// the same `o` then reads its file.
    pub(crate) fn sheet_mouse(&mut self, mouse: crate::app::input::MouseInput) {
        use crate::app::input::{MouseButton, MouseEventKind};
        let pos = (mouse.col, mouse.row);
        let (frame, body) = (self.sheet_hits.frame, self.sheet_hits.body);
        let hit =
            |r: ratatui::layout::Rect| r.contains(ratatui::layout::Position { x: pos.0, y: pos.1 });
        // An opened report renders in the unified top-level Reader (`R3-M2`):
        // wheel scrolls it, clicks outside Reader close only the Reader.
        if self.sheet.as_ref().is_some_and(|s| s.view.is_some()) {
            let reader_area = self.hollywood_hits.view.as_ref().map(|v| v.area);
            let max_scroll = self
                .hollywood_hits
                .view
                .as_ref()
                .and_then(|v| v.metrics)
                .map(|m| m.max_offset_from_bottom);
            match mouse.kind {
                MouseEventKind::ScrollDown | MouseEventKind::ScrollUp => {
                    let delta: isize = if mouse.kind == MouseEventKind::ScrollDown {
                        3
                    } else {
                        -3
                    };
                    if let Some(view) = self.sheet.as_mut().and_then(|s| s.view.as_mut()) {
                        let next = view.scroll.saturating_add_signed(delta);
                        view.scroll = max_scroll.map_or(next, |max| next.min(max));
                    }
                }
                MouseEventKind::Down(MouseButton::Left) if reader_area.is_none_or(|r| !hit(r)) => {
                    if let Some(sheet) = self.sheet.as_mut() {
                        sheet.view = None;
                    }
                }
                _ => {}
            }
            return;
        }
        match mouse.kind {
            MouseEventKind::Down(MouseButton::Left) if !hit(frame) => {
                self.sheet = None;
            }
            MouseEventKind::ScrollDown | MouseEventKind::ScrollUp if hit(body) || hit(frame) => {
                let delta: isize = if mouse.kind == MouseEventKind::ScrollDown {
                    3
                } else {
                    -3
                };
                self.sheet_move(delta);
            }
            MouseEventKind::Down(MouseButton::Left) if hit(body) => {
                let summary = self.sheet_summary();
                let Some(sheet) = self.sheet.as_mut() else {
                    return;
                };
                let lines = lines_for(sheet, summary.as_ref());
                let row = sheet.scroll + (pos.1 - body.y) as usize;
                if lines.get(row).is_some_and(|l| l.selectable) {
                    sheet.sel = row;
                }
            }
            _ => {}
        }
    }

    /// Move the sheet's selection one selectable line — separators
    /// are skipped like the dock's dividers (S2 §4).
    fn sheet_move(&mut self, delta: isize) {
        let summary = self.sheet_summary();
        let Some(sheet) = self.sheet.as_mut() else {
            return;
        };
        let lines = lines_for(sheet, summary.as_ref());
        let mut sel = sheet.sel;
        for _ in 0..delta.unsigned_abs() {
            let next = if delta > 0 {
                (sel + 1..lines.len()).find(|&i| lines[i].selectable)
            } else {
                (0..sel).rev().find(|&i| lines[i].selectable)
            };
            match next {
                Some(next) => sel = next,
                None => break,
            }
        }
        sheet.sel = sel.min(lines.len().saturating_sub(1));
        let body = self.sheet_hits.body.height.max(1) as usize;
        if sheet.sel < sheet.scroll {
            sheet.scroll = sheet.sel;
        } else if sheet.sel >= sheet.scroll + body {
            sheet.scroll = sheet.sel + 1 - body;
        }
    }

    /// Page the sheet's content — the selection follows the scroll.
    fn sheet_page(&mut self, delta: isize) {
        let page = self.sheet_hits.body.height.max(1) as isize;
        self.sheet_move(delta.saturating_mul(page));
    }

    /// The sheet's studio summary — the instant detail the tabs show
    /// while the ledger reads are out.
    fn sheet_summary(&self) -> Option<backlot_model::summary::StudioSummary> {
        let sheet = self.sheet.as_ref()?;
        self.panel_summaries()
            .into_iter()
            .flat_map(|m| m.summary.studios.into_iter())
            .find(|s| s.id == sheet.studio.id)
    }
}
