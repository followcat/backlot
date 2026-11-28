//! The S3 native dialogs' app side (§2): open, key and mouse
//! dispatch, the revise prefill fetch (a local read or `backlot
//! report show` through the runner on a remote studio), `^E`'s
//! `$EDITOR` popup and the submits — each a `run_action` call so
//! pending, toast and failure all ride the one path.

use crate::studio_dialog;
use crate::studio_dialog::{BodyEditor, DialogField, ReviseDialog, StudioDialog, TellDialog};
use crate::studio_menu::StudioMenu;

use super::App;

mod approval;
mod design_sign;
mod keys;
mod lifecycle;
mod permissions;
mod restore;

impl App {
    /// `Tell <name>…` — `Tell alice · acme-shop · host-b`.
    pub(crate) fn tell_dialog_open(&mut self, menu: &StudioMenu) {
        let Some(key) = menu.studio.clone() else {
            return;
        };
        let role = menu
            .role
            .as_ref()
            .map(|r| (r.role.clone(), r.name.clone()))
            .unwrap_or_else(|| ("producer".to_owned(), "producer".to_owned()));
        self.studio_dialog = Some(StudioDialog::Tell(TellDialog {
            title: format!(
                "Tell {} · {}",
                role.1,
                studio_dialog::StudioDialog::place(&key)
            ),
            key,
            role: role.0,
            body: BodyEditor::default(),
            field: DialogField::Body,
            error: None,
        }));
    }

    /// `Revise requirement…` — opens at once; when `from > 0`, the
    /// prefill fetch reads the studio's real `req/…` file from the
    /// state dir via `backlot report show` on both local and remote
    /// studios. When `from == 0`, opens an empty editor with no fetch.
    pub(crate) fn revise_dialog_open(&mut self, menu: &StudioMenu) {
        let Some(key) = menu.studio.clone() else {
            return;
        };
        let (from, to) = (menu.req_version, menu.req_version + 1);
        let local = key.machine.is_none();
        let file = if from == 0 {
            None
        } else {
            Some(
                menu.req_file
                    .clone()
                    .or_else(|| {
                        let machines = self.panel_summaries();
                        crate::panel::data::studio(&machines, &key)
                            .and_then(|s| s.studio.req_file.clone())
                    })
                    .unwrap_or_else(|| format!("req/v{from}.md")),
            )
        };
        self.studio_dialog = Some(StudioDialog::Revise(ReviseDialog {
            title: format!(
                "Revise requirement (v{from} → v{to}) · {}",
                studio_dialog::StudioDialog::place(&key)
            ),
            key: key.clone(),
            body: (from == 0).then(BodyEditor::default),
            summary: crate::chrome::text_editor::TextEditor::new("", false),
            field: DialogField::Body,
            local,
            error: None,
            prefill_failed: false,
            confirm_overwrite: false,
            editor_gen: 0,
        }));
        if let Some(file) = file {
            self.revise_prefill(&key, &file);
        } else {
            self.studio_dialog_gen += 1;
        }
    }

    /// The prefill fetch — reads the studio state directory's `req/…`
    /// artifact via `backlot report show` on both local and remote
    /// studios, off the input loop. A stale generation drops late
    /// replies.
    fn revise_prefill(&mut self, key: &crate::studio::StudioKey, file: &str) {
        self.studio_dialog_gen += 1;
        let epoch = self.studio_dialog_gen;
        let tx = self.dialog_fetch_tx.clone();
        let run = self.ns_runner();
        let machine = key.machine.clone();
        let argv = vec![
            "report".into(),
            "show".into(),
            "--studio".into(),
            key.id.clone(),
            "--file".into(),
            file.to_owned(),
        ];
        tokio::spawn(async move {
            let _ = tx.send((epoch, run(machine, argv, None).await)).await;
        });
    }

    /// The prefill/dry-run reply — only while the matching dialog is
    /// still open does it land; a closed or re-opened dialog's
    /// generation drops the fetch.
    pub(crate) fn on_dialog_fetch(&mut self, (epoch, result): (u64, Result<String, String>)) {
        if epoch != self.studio_dialog_gen {
            return;
        }
        // The lifecycle dialogs own their fetches (the recast runner
        // probe and the restore dry run).
        if self.lifecycle_dialog_fetch(result.clone()) {
            return;
        }
        // `Edit cast…`'s `studio cast set --dry-run` lands here too.
        if let Some(StudioDialog::CastEdit(d)) = self.studio_dialog.as_mut()
            && d.step == crate::studio_dialog::CastEditStep::Checking
        {
            d.dry_run(result);
            self.sched.mark_dirty(std::time::Instant::now());
            return;
        }
        match self.studio_dialog.as_mut() {
            Some(StudioDialog::Revise(d)) => {
                match result {
                    Ok(text) => {
                        d.body = Some(BodyEditor::new(&text));
                        d.prefill_failed = false;
                        d.confirm_overwrite = false;
                        d.error = None;
                    }
                    Err(error) => {
                        d.body = Some(BodyEditor::default());
                        d.prefill_failed = true;
                        d.confirm_overwrite = false;
                        d.error = Some(error);
                    }
                }
                self.sched.mark_dirty(std::time::Instant::now());
            }
            Some(StudioDialog::Confirm(d))
                if matches!(
                    d.for_,
                    crate::studio_dialog::ConfirmFor::DeleteStudio { force: false, .. }
                ) =>
            {
                self.delete_studio_dry_run_reply(result);
            }
            Some(StudioDialog::Confirm(d))
                if matches!(d.for_, crate::studio_dialog::ConfirmFor::Prune { .. }) =>
            {
                self.prune_dry_run_reply(result);
            }
            _ => {}
        }
    }

    /// `^E` in the Revise dialog — a temp copy in `$VISUAL`/`$EDITOR`
    /// in a floating terminal (local studios only, §2.4); the file
    /// is re-read when the editor exits.
    fn revise_editor(&mut self) {
        let Some(StudioDialog::Revise(d)) = self.studio_dialog.as_mut() else {
            return;
        };
        if !d.local {
            return;
        }
        d.editor_gen += 1;
        let epoch = d.editor_gen;
        let text = d.body.as_ref().map(|b| b.text()).unwrap_or_default();
        let file = std::env::temp_dir().join(format!("backlot-req-{}.md", std::process::id()));
        if std::fs::write(&file, text).is_err() {
            if let Some(StudioDialog::Revise(d)) = self.studio_dialog.as_mut() {
                d.error = Some("cannot write the requirement copy".to_owned());
            }
            return;
        }
        self.studio_editor_file = Some((file.clone(), epoch));
        let line = format!(
            "${{VISUAL:-${{EDITOR:-vi}}}} {}",
            sh_quote(&file.to_string_lossy())
        );
        if self.popup.is_some() || self.popup_pending.is_some() {
            self.dismiss_popup();
        }
        let mut params = hc_proto::terminal::RunParams {
            line: Some(line),
            rm: true,
            keep: false,
            no_notify: true,
            no_parent: true,
            env: super::creation::program_env(),
            labels: std::collections::BTreeMap::from([(
                super::popup::POPUP_LABEL.into(),
                "1".into(),
            )]),
            display: self.host_facts.display(),
            ..Default::default()
        };
        if let Some(geometry) = crate::popup_size::resolve_popup_geometry(
            Some(crate::popup_size::PopupSize::Percent(80)),
            Some(crate::popup_size::PopupSize::Percent(80)),
            self.frame_area(),
        ) {
            params.size = Some(hc_proto::terminal::InitialSize {
                cols: geometry.terminal_cols,
                rows: geometry.terminal_rows,
            });
        }
        self.popup_pending = Some(super::popup::PopupPending {
            width: Some(crate::popup_size::PopupSize::Percent(80)),
            height: Some(crate::popup_size::PopupSize::Percent(80)),
            persistent: false,
            owner: String::new(),
            command: String::new(),
            install_machine: None,
        });
        self.submit_action(crate::operations::Action::RunPopup { params });
    }

    /// The revise `$EDITOR` popup closed — re-read the temp copy
    /// into the body, like the cast editor's reload (U6b).
    pub(crate) fn studio_editor_check(&mut self) {
        let Some((file, epoch)) = self.studio_editor_file.clone() else {
            return;
        };
        if self.popup.is_some() || self.popup_pending.is_some() {
            return;
        }
        self.studio_editor_file = None;
        // `Edit cast…`'s editor rides the same popup: its close runs the
        // dry run on the edited file, never the Revise reload.
        if matches!(
            self.studio_dialog.as_ref(),
            Some(StudioDialog::CastEdit(d)) if d.epoch == epoch
        ) {
            self.cast_edit_editor_closed(&file, epoch);
            let _ = std::fs::remove_file(&file);
            return;
        }
        let live = matches!(
            self.studio_dialog.as_ref(),
            Some(StudioDialog::Revise(d)) if d.editor_gen == epoch
        );
        match std::fs::read_to_string(&file) {
            Ok(text) if live => {
                if let Some(StudioDialog::Revise(d)) = self.studio_dialog.as_mut() {
                    d.body = Some(BodyEditor::new(&text));
                }
            }
            Err(_) if live => {
                if let Some(StudioDialog::Revise(d)) = self.studio_dialog.as_mut() {
                    d.error = Some("the requirement copy vanished".to_owned());
                }
            }
            _ => {}
        }
        let _ = std::fs::remove_file(&file);
        self.sched.mark_dirty(std::time::Instant::now());
    }
}

/// POSIX single-quote escaping — the `$EDITOR` line's file path.
pub(super) fn sh_quote(word: &str) -> String {
    format!("'{}'", word.replace('\'', "'\\''"))
}
