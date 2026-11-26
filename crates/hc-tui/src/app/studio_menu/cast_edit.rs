//! `Edit cast…` app side: fetch the effective cast, open it in the
//! person's `$VISUAL`/`$EDITOR`, check it with `studio cast set
//! --dry-run`, then record it on confirm. The dialog model is
//! `studio_dialog::CastEditDialog`; this file owns the fetch, the editor
//! round trip and the submits, the way `prune.rs` owns its plan.

use std::time::Instant;

use crate::app::App;
use crate::studio::StudioKey;
use crate::studio_dialog::{CastEditDialog, CastEditStep, StudioDialog};

impl App {
    /// `Edit cast…` — open the dialog in its loading state and fetch the
    /// effective cast (`studio cast show`).
    pub(crate) fn cast_edit_open(&mut self, key: &StudioKey) {
        self.studio_dialog_gen += 1;
        let epoch = self.studio_dialog_gen;
        let mut d = CastEditDialog::new(key);
        d.epoch = epoch;
        self.studio_dialog = Some(StudioDialog::CastEdit(d));
        self.run_action(crate::studio_cmds::cast_show(key));
    }

    /// Write the cast to a temp file and open it in `$VISUAL`/`$EDITOR`
    /// in a floating terminal, like the Revise dialog's `^E`.
    fn cast_edit_open_editor(&mut self, key: &StudioKey, text: String) {
        let file = std::env::temp_dir().join(format!(
            "backlot-cast-{}-{}.toml",
            std::process::id(),
            self.studio_dialog_gen
        ));
        if std::fs::write(&file, &text).is_err() {
            if let Some(StudioDialog::CastEdit(d)) = self.studio_dialog.as_mut() {
                d.failed("cannot write the cast copy".to_owned());
            }
            return;
        }
        let epoch = self.studio_dialog_gen;
        self.studio_editor_file = Some((file.clone(), epoch));
        let line = format!(
            "${{VISUAL:-${{EDITOR:-vi}}}} {}",
            crate::app::studio_dialog::sh_quote(&file.to_string_lossy())
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
            env: crate::app::creation::program_env(),
            labels: std::collections::BTreeMap::from([(
                crate::app::popup::POPUP_LABEL.into(),
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
        self.popup_pending = Some(crate::app::popup::PopupPending {
            width: Some(crate::popup_size::PopupSize::Percent(80)),
            height: Some(crate::popup_size::PopupSize::Percent(80)),
            persistent: false,
            owner: String::new(),
            command: String::new(),
            install_machine: None,
        });
        let _ = key;
        self.submit_action(crate::operations::Action::RunPopup { params });
    }

    /// The editor closed: read the temp copy back and check it with
    /// `studio cast set --dry-run`. Keyed on the dialog's generation so
    /// a closed dialog's file is never re-read.
    pub(crate) fn cast_edit_editor_closed(&mut self, file: &std::path::Path, epoch: u64) {
        let live = matches!(
            self.studio_dialog.as_ref(),
            Some(StudioDialog::CastEdit(d)) if d.epoch == epoch && d.step == CastEditStep::Editing
        );
        if !live {
            return;
        }
        let text = match std::fs::read_to_string(file) {
            Ok(t) => t,
            Err(_) => {
                if let Some(StudioDialog::CastEdit(d)) = self.studio_dialog.as_mut() {
                    d.dry_run(Err("the cast copy vanished".to_owned()));
                }
                return;
            }
        };
        if let Some(StudioDialog::CastEdit(d)) = self.studio_dialog.as_mut() {
            d.cast = text.clone();
            d.checking();
        }
        self.cast_edit_dry_run(&text);
    }

    /// `studio cast set --file - --dry-run` with the edited content.
    fn cast_edit_dry_run(&mut self, text: &str) {
        let Some(StudioDialog::CastEdit(d)) = self.studio_dialog.as_ref() else {
            return;
        };
        let key = d.key.clone();
        let epoch = d.epoch;
        let tx = self.dialog_fetch_tx.clone();
        let run = self.ns_runner();
        let machine = key.machine.clone();
        let argv = vec![
            "studio".into(),
            "cast".into(),
            "set".into(),
            "--file".into(),
            "-".into(),
            "--dry-run".into(),
            "--studio".into(),
            key.id.clone(),
            "--as".into(),
            "user".into(),
        ];
        let body = text.to_owned();
        tokio::spawn(async move {
            let _ = tx.send((epoch, run(machine, argv, Some(body)).await)).await;
        });
    }

    /// Apply the confirmed cast: `studio cast set --file -`.
    pub(crate) fn cast_edit_apply(&mut self, d: &CastEditDialog) {
        self.run_action(crate::studio_cmds::cast_set(&d.key, d.cast.clone(), false));
        self.studio_dialog = None;
        self.sched.mark_dirty(Instant::now());
    }

    /// Test seam: the `$EDITOR` closed with `text` — set the edited cast
    /// and run the dry run, exactly as `cast_edit_editor_closed` does
    /// after reading the temp file. The real editor is a popup that
    /// needs an operations channel; the tests drive the same steps
    /// without one.
    #[cfg(test)]
    pub(crate) fn cast_edit_edited(&mut self, text: &str) {
        // The real editor is a popup that needs an operations channel;
        // drop its pending request and the temp copy the open left, so
        // the dialog's own keys still parse and TMPDIR stays clean.
        self.popup_pending = None;
        if let Some((file, _)) = self.studio_editor_file.take() {
            let _ = std::fs::remove_file(file);
        }
        let Some(StudioDialog::CastEdit(d)) = self.studio_dialog.as_mut() else {
            return;
        };
        d.cast = text.to_owned();
        d.checking();
        self.cast_edit_dry_run(text);
    }

    /// `Edit cast…` keys: `Esc` cancels; on a ready diff `Enter`/`y`
    /// applies. While loading or checking, only `Esc` escapes.
    pub(crate) fn cast_edit_key(
        &mut self,
        d: CastEditDialog,
        code: crossterm::event::KeyCode,
        modifiers: crossterm::event::KeyModifiers,
    ) -> Option<StudioDialog> {
        use crossterm::event::KeyCode;
        if matches!(code, KeyCode::Esc) {
            return None;
        }
        let confirm = matches!(code, KeyCode::Enter)
            || (matches!(code, KeyCode::Char('y')) && modifiers.is_empty());
        if confirm && d.ready && !d.busy {
            self.cast_edit_apply(&d);
            return None;
        }
        Some(StudioDialog::CastEdit(d))
    }

    /// The `studio cast show` reply routed to the open `Edit cast…`
    /// dialog: its text opens the editor. A reply for any other action
    /// (or with the dialog closed) is left to the usual toast path.
    pub(crate) fn cast_show_reply(
        &mut self,
        pending: &crate::action::Pending,
        result: &Result<String, String>,
    ) -> bool {
        let Some(studio) = pending.action.mark.strip_prefix("cast:") else {
            return false;
        };
        let Some(StudioDialog::CastEdit(d)) = self.studio_dialog.as_mut() else {
            return false;
        };
        // Marks are `cast:<machine>:<id>`; only the studio we are editing
        // may answer, and only while the fetch is live.
        let suffix = format!(":{}", d.key.id);
        if !studio.ends_with(&suffix) || d.step != CastEditStep::Loading {
            return false;
        }
        let key = d.key.clone();
        match result {
            Ok(text) if text.trim().is_empty() => d.failed("the studio's cast is empty".to_owned()),
            Ok(text) => {
                d.editing(text.clone());
                self.cast_edit_open_editor(&key, text.clone());
            }
            Err(err) => d.failed(err.clone()),
        }
        self.sched.mark_dirty(Instant::now());
        true
    }
}
