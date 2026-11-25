//! Task & Delivery Sheet artifact reader fetching, sibling file stepping,
//! and Reader metadata derivation (`■ CHECK` / `■ DELIVERY` / `■ REQ`).

use super::App;
use super::sheet::SheetReply;
use crate::hollywood::{ReaderKind, ReaderMeta, ReaderOrigin};
use crate::sheet::{SheetTab, lines_for};

impl App {
    /// `⏎` inside the sheet: the selected line's file streams
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
        self.sheet_fetch_line(sheet.sel, false);
    }

    /// Step backward (`delta = -1`) or forward (`delta = 1`) across file-backed
    /// rows on the `Deliveries` tab, keeping `sheet.sel` synchronized live.
    pub(crate) fn sheet_step_file(&mut self, delta: isize) {
        let summary = self.sheet_summary();
        let next_sel = {
            let Some(sheet) = self.sheet.as_ref() else {
                return;
            };
            if sheet.tab != SheetTab::Deliveries {
                return;
            }
            let lines = lines_for(sheet, summary.as_ref());
            let file_rows: Vec<usize> = lines
                .iter()
                .enumerate()
                .filter_map(|(i, l)| l.file.is_some().then_some(i))
                .collect();
            let Some(cur) = file_rows.iter().position(|&idx| idx == sheet.sel) else {
                return;
            };
            let target_pos = if delta < 0 {
                cur.checked_sub(delta.unsigned_abs())
            } else if delta > 0 {
                cur.checked_add(delta as usize)
            } else {
                None
            };
            target_pos.and_then(|p| file_rows.get(p).copied())
        };
        let Some(next_sel) = next_sel else {
            return;
        };
        if let Some(sheet) = self.sheet.as_mut() {
            sheet.sel = next_sel;
        }
        self.sheet_fetch_line(next_sel, true);
    }

    fn sheet_fetch_line(&mut self, sel: usize, show_loading: bool) {
        let summary = self.sheet_summary();
        let Some(sheet) = self.sheet.as_ref() else {
            return;
        };
        let lines = lines_for(sheet, summary.as_ref());
        let Some(line) = lines.get(sel) else {
            return;
        };
        let Some(file) = line.file.clone() else {
            return;
        };
        let file_rows: Vec<usize> = lines
            .iter()
            .enumerate()
            .filter_map(|(i, l)| l.file.is_some().then_some(i))
            .collect();
        let nav_index = file_rows.iter().position(|&idx| idx == sel).map(|p| p + 1);
        let nav_total = (!file_rows.is_empty()).then_some(file_rows.len());
        let kind = if file.starts_with("checks/")
            || line.meta.as_ref().is_some_and(|m| m.check_status.is_some())
        {
            ReaderKind::Check
        } else if file.starts_with("req/") {
            ReaderKind::Req
        } else {
            ReaderKind::Delivery
        };
        let meta = ReaderMeta {
            kind,
            studio: Some(sheet.studio.id.clone()),
            machine: sheet.studio.machine.clone(),
            task: line
                .meta
                .as_ref()
                .and_then(|m| m.task.clone())
                .or_else(|| sheet.task.clone()),
            file: Some(file.clone()),
            check_status: line.meta.as_ref().and_then(|m| m.check_status.clone()),
            nav_index,
            nav_total,
            origin: ReaderOrigin::SheetDeliveries(sheet.studio.clone(), sheet.task.clone()),
            ..ReaderMeta::default()
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
        if show_loading && let Some(sv) = sheet_mut.view.as_mut() {
            let short = file.rsplit('/').next().unwrap_or(&file).to_string();
            sv.meta.loading_label = Some(short);
        }
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
                    meta: Box::new(meta),
                    out,
                })
                .await;
        });
    }

    /// The sheet's studio summary — the instant detail the tabs show
    /// while the ledger reads are out.
    pub(super) fn sheet_summary(&self) -> Option<backlot_model::summary::StudioSummary> {
        let sheet = self.sheet.as_ref()?;
        self.panel_summaries()
            .into_iter()
            .flat_map(|m| m.summary.studios.into_iter())
            .find(|s| s.id == sheet.studio.id)
    }
}
