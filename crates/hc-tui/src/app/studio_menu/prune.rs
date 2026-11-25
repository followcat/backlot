//! `Free work areas (prune)…` (L7): the dry-run plan first, then the
//! real `studio prune`. The ledger, reports and messages stay.

use std::time::Instant;

use crate::action::{FAIL_HOLD, fail_body};
use crate::app::App;
use crate::app::action_tracker::ActionToast;
use crate::studio::StudioKey;
use crate::studio_dialog::{ConfirmFor, StudioDialog};

impl App {
    /// Open the confirm in its loading state and fetch
    /// `studio prune <studio> --dry-run --as user`.
    pub(crate) fn prune_open(&mut self, key: &StudioKey) {
        self.studio_dialog = Some(StudioDialog::Confirm(crate::studio_dialog::ConfirmDialog {
            title: format!(
                "Free work areas · {}",
                crate::studio_dialog::StudioDialog::place(key)
            ),
            body: "Inspecting the studio's work areas…".to_owned(),
            loading: true,
            error: None,
            for_: ConfirmFor::Prune {
                key: key.clone(),
                token: None,
            },
        }));
        self.studio_dialog_gen += 1;
        let epoch = self.studio_dialog_gen;
        let tx = self.dialog_fetch_tx.clone();
        let run = self.ns_runner();
        let machine = key.machine.clone();
        let mut argv = vec![
            "studio".into(),
            "prune".into(),
            key.id.clone(),
            "--dry-run".into(),
            "--as".into(),
            "user".into(),
        ];
        if self.server_mode() && key.machine.is_none() {
            argv.push(crate::server_action::PLAN_FLAG.into());
        }
        tokio::spawn(async move {
            let _ = tx.send((epoch, run(machine, argv, None).await)).await;
        });
    }

    /// The dry-run plan landed — show it verbatim; a failed read closes
    /// the dialog with the machine's own words.
    pub(in crate::app) fn prune_dry_run_reply(&mut self, result: Result<String, String>) {
        let now = Instant::now();
        let Some(StudioDialog::Confirm(d)) = self.studio_dialog.as_mut() else {
            return;
        };
        let key = match &d.for_ {
            ConfirmFor::Prune { key, .. } => key.clone(),
            _ => return,
        };
        match result {
            Ok(stdout) => {
                d.loading = false;
                let (stdout, token) = crate::server_action::split_plan_reply(&stdout);
                let plan = stdout.trim();
                d.body = if plan.is_empty() {
                    "nothing to prune — no worktrees or branches are left".to_owned()
                } else {
                    format!("{plan}\n\nThe ledger, reports and messages stay.")
                };
                if let ConfirmFor::Prune { token: slot, .. } = &mut d.for_ {
                    *slot = token;
                }
            }
            Err(err) => {
                self.studio_dialog = None;
                let action = crate::studio_cmds::prune(&key, None);
                let title = match &action.machine {
                    Some(machine) => format!("{machine} failed: {}", action.name),
                    None => format!("failed: {}", action.name),
                };
                self.action_toast = Some(ActionToast {
                    title,
                    body: fail_body(&err),
                    fail: true,
                    command: None,
                    start_stage: None,
                    deadline: now + FAIL_HOLD,
                });
            }
        }
        self.sched.mark_dirty(now);
    }
}
