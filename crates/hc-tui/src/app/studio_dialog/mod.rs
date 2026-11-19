//! The S3 native dialogs' app side (§2): open, key and mouse
//! dispatch, the revise prefill fetch (a local read or `backlot
//! report show` through the runner on a remote studio), `^E`'s
//! `$EDITOR` popup and the submits — each a `run_action` call so
//! pending, toast and failure all ride the one path.

use crate::studio_dialog;
use crate::studio_dialog::{BodyEditor, DialogField, ReviseDialog, StudioDialog, TellDialog};
use crate::studio_menu::StudioMenu;

use super::App;

mod keys;

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

    /// `Revise requirement…` — opens at once with the body loading;
    /// the prefill fetch fills it (`requirements/v<n>.md` — a local
    /// read on this core, `backlot report show` on a remote studio).
    pub(crate) fn revise_dialog_open(&mut self, menu: &StudioMenu) {
        let Some(key) = menu.studio.clone() else {
            return;
        };
        let (from, to) = (menu.req_version, menu.req_version + 1);
        let file = format!("requirements/v{from}.md");
        let local = key.machine.is_none();
        self.studio_dialog = Some(StudioDialog::Revise(ReviseDialog {
            title: format!(
                "Revise requirement (v{from} → v{to}) · {}",
                studio_dialog::StudioDialog::place(&key)
            ),
            key: key.clone(),
            body: None,
            summary: crate::chrome::text_editor::TextEditor::new("", false),
            field: DialogField::Body,
            local,
            error: None,
            editor_gen: 0,
        }));
        self.revise_prefill(&key, &file);
    }

    /// The prefill fetch — the local read happens off the loop like
    /// the remote call, so a slow disk never stalls input. A stale
    /// generation drops the reply (the dialog may be gone).
    fn revise_prefill(&mut self, key: &crate::studio::StudioKey, file: &str) {
        self.studio_dialog_gen += 1;
        let epoch = self.studio_dialog_gen;
        let tx = self.dialog_fetch_tx.clone();
        if let Some(machine) = key.machine.clone() {
            let run = self.ns_runner();
            let argv = vec![
                "report".into(),
                "show".into(),
                "--studio".into(),
                key.id.clone(),
                "--file".into(),
                file.to_owned(),
            ];
            tokio::spawn(async move {
                let _ = tx.send((epoch, run(Some(machine), argv, None).await)).await;
            });
        } else {
            let machines = self.panel_summaries();
            let dir =
                crate::panel::data::studio(&machines, key).and_then(|s| s.studio.repo.clone());
            let file = file.to_owned();
            tokio::spawn(async move {
                let result = match dir {
                    Some(dir) => std::fs::read_to_string(std::path::Path::new(&dir).join(&file))
                        .map_err(|e| format!("cannot read {}: {e}", file)),
                    None => Err("the studio has no repo path".to_owned()),
                };
                let _ = tx.send((epoch, result)).await;
            });
        }
    }

    /// The prefill reply — only while the matching Revise dialog is
    /// still open does it land; a closed or re-opened dialog's
    /// generation drops the fetch.
    pub(crate) fn on_dialog_fetch(&mut self, (epoch, result): (u64, Result<String, String>)) {
        if epoch != self.studio_dialog_gen {
            return;
        }
        let Some(StudioDialog::Revise(d)) = self.studio_dialog.as_mut() else {
            return;
        };
        match result {
            Ok(text) => d.body = Some(BodyEditor::new(&text)),
            Err(error) => {
                d.body = Some(BodyEditor::default());
                d.error = Some(error);
            }
        }
        self.sched.mark_dirty(std::time::Instant::now());
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
fn sh_quote(word: &str) -> String {
    format!("'{}'", word.replace('\'', "'\\''"))
}
