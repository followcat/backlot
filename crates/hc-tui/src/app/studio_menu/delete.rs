//! App wiring for `Delete studio…`: dry-run inspection dialog, real
//! delete submission, real outcome toasts, `--force` second confirm,
//! and immediate in-memory eviction from every surface.

use std::sync::Arc;
use std::time::Instant;

use backlot_stage::DeleteReport;
use hc_proto::watch::DocKey;

use crate::action::{DONE_HOLD, FAIL_HOLD, Pending, copy_line, done_title, fail_body, fail_title};
use crate::app::App;
use crate::app::action_tracker::ActionToast;
use crate::studio::{StudioFocus, StudioKey};
use crate::studio_dialog::StudioDialog;

impl App {
    /// Open the first confirmation dialog immediately in a loading state
    /// and fetch `studio delete <id> --dry-run --json --as user`.
    pub(crate) fn delete_studio_open(&mut self, key: &StudioKey) {
        self.studio_dialog = Some(StudioDialog::delete_studio_loading(key));
        self.studio_dialog_gen += 1;
        let epoch = self.studio_dialog_gen;
        let tx = self.dialog_fetch_tx.clone();
        let run = self.ns_runner();
        let machine = key.machine.clone();
        let argv = vec![
            "studio".into(),
            "delete".into(),
            key.id.clone(),
            "--dry-run".into(),
            "--json".into(),
            "--as".into(),
            "user".into(),
        ];
        tokio::spawn(async move {
            let _ = tx.send((epoch, run(machine, argv, None).await)).await;
        });
    }

    /// Populate the first confirmation dialog from the `--dry-run --json`
    /// reply, or close it and show an error toast on failure.
    pub(in crate::app) fn delete_studio_dry_run_reply(&mut self, result: Result<String, String>) {
        let now = Instant::now();
        let Some(StudioDialog::Confirm(d)) = self.studio_dialog.as_mut() else {
            return;
        };
        let key = match &d.for_ {
            crate::studio_dialog::ConfirmFor::DeleteStudio { key, force: false } => key.clone(),
            _ => return,
        };
        match result {
            Ok(stdout) => match parse_delete_report(&stdout) {
                Some(report) => {
                    d.loading = false;
                    d.body = format_delete_confirm(&report);
                }
                None => {
                    self.studio_dialog = None;
                    self.action_toast = Some(ActionToast {
                        title: fail_title_for(
                            key.machine.as_deref(),
                            &format!("inspect {}", key.id),
                        ),
                        body: "invalid dry-run response from backlot studio delete".into(),
                        fail: true,
                        command: None,
                        start_stage: None,
                        deadline: now + FAIL_HOLD,
                    });
                }
            },
            Err(err) => {
                self.studio_dialog = None;
                self.action_toast = Some(ActionToast {
                    title: fail_title_for(key.machine.as_deref(), &format!("inspect {}", key.id)),
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

    /// Handle replies for `studio delete`: never show a fixed success text
    /// unconditionally; evict immediately only when `archived == true`, and
    /// open the `--force` confirmation when kept items block archiving.
    pub(in crate::app) fn delete_studio_reply(
        &mut self,
        pending: &Pending,
        result: &Result<String, String>,
    ) -> bool {
        if !pending
            .action
            .argv
            .starts_with(&["studio".into(), "delete".into()])
        {
            return false;
        }
        let now = Instant::now();
        let Some(id) = pending.action.argv.get(2).cloned() else {
            return false;
        };
        let key = StudioKey {
            machine: pending.action.machine.clone(),
            id,
        };
        let force = pending.action.argv.iter().any(|a| a == "--force");
        match result {
            Ok(stdout) => match parse_delete_report(stdout) {
                Some(report) if report.archived => {
                    self.evict_deleted_studio(&key);
                    self.action_toast = Some(ActionToast {
                        title: done_title(pending),
                        body: String::new(),
                        fail: false,
                        command: None,
                        start_stage: None,
                        deadline: now + DONE_HOLD,
                    });
                }
                Some(report) => {
                    let body = if report.kept.is_empty() {
                        "studio was not archived".to_owned()
                    } else {
                        report.kept.join("; ")
                    };
                    self.action_toast = Some(ActionToast {
                        title: fail_title(pending),
                        body,
                        fail: true,
                        command: Some(copy_line(pending)),
                        start_stage: None,
                        deadline: now + FAIL_HOLD,
                    });
                }
                None => {
                    self.action_toast = Some(ActionToast {
                        title: fail_title(pending),
                        body: "invalid JSON output from studio delete".to_owned(),
                        fail: true,
                        command: Some(copy_line(pending)),
                        start_stage: None,
                        deadline: now + FAIL_HOLD,
                    });
                }
            },
            Err(err) => {
                if let Some(report) = parse_delete_report(err)
                    && !report.kept.is_empty()
                    && !report.archived
                {
                    self.action_toast = Some(ActionToast {
                        title: fail_title(pending),
                        body: report.kept.join("; "),
                        fail: true,
                        command: Some(copy_line(pending)),
                        start_stage: None,
                        deadline: now + FAIL_HOLD,
                    });
                    if !force {
                        self.studio_dialog =
                            Some(StudioDialog::delete_studio_force(&key, &report.kept));
                    }
                } else {
                    self.action_toast = Some(ActionToast {
                        title: fail_title(pending),
                        body: fail_body(err),
                        fail: true,
                        command: Some(copy_line(pending)),
                        start_stage: None,
                        deadline: now + FAIL_HOLD,
                    });
                }
            }
        }
        true
    }

    /// Remove `key` immediately from in-memory summaries, drafts,
    /// remembered roles, non-shell studio terminals, and move focus to the
    /// next remaining studio (or `None`).
    fn evict_deleted_studio(&mut self, key: &StudioKey) {
        let machines = self.panel_summaries();
        let studios = crate::panel::data::studios(&machines);
        let pos = studios.iter().position(|s| &s.key() == key);
        let remaining: Vec<StudioKey> = studios
            .iter()
            .map(|s| s.key())
            .filter(|k| k != key)
            .collect();

        let doc_key = DocKey {
            machine: key.machine.clone(),
            name: "backlot".into(),
        };
        if let Some(doc) = self.view.documents_mut().get_mut(&doc_key) {
            let mut value = doc.value.clone();
            if let Some(arr) = value.get_mut("studios").and_then(|s| s.as_array_mut()) {
                arr.retain(|s| s.get("id").and_then(|v| v.as_str()) != Some(key.id.as_str()));
            }
            Arc::make_mut(doc).value = value;
        }

        let seq = self.view.inventory.seq();
        let kept_terms: Vec<_> = self
            .view
            .inventory
            .ordered_terminals()
            .filter(|t| {
                !(crate::project::machine_of(&t.address) == key.machine.as_deref()
                    && t.labels.get("backlot.studio").is_some_and(|s| s == &key.id)
                    && !t.labels.contains_key("backlot.shell")
                    && !matches!(
                        t.labels.get("backlot.role").map(String::as_str),
                        Some("stage" | "hollywood")
                    ))
            })
            .cloned()
            .collect();
        if kept_terms.len() != self.view.inventory.terminals().len() {
            self.view.inventory_mut().replace(seq, kept_terms);
        }

        self.studio.remembered.remove(key);
        self.panel.clear_draft(key);
        if self.panel.finish_confirm.as_ref() == Some(key) {
            self.panel.finish_confirm = None;
        }
        if self.sheet.as_ref().is_some_and(|s| &s.studio == key) {
            self.sheet = None;
        }

        if remaining.is_empty() {
            self.studio.current = None;
            if matches!(
                &self.studio.focus,
                Some(StudioFocus::Studio(k) | StudioFocus::Role(k, _)) if k == key
            ) {
                self.studio.focus = None;
            }
        } else {
            let next_key = remaining[pos.map(|i| i % remaining.len()).unwrap_or(0)].clone();
            if self.studio.current.as_ref() == Some(key) {
                self.studio.current = Some(next_key.clone());
            }
            if matches!(
                &self.studio.focus,
                Some(StudioFocus::Studio(k) | StudioFocus::Role(k, _)) if k == key
            ) {
                self.studio.focus = Some(StudioFocus::Studio(next_key));
            }
        }
        self.reconcile_published_view();
    }
}

fn fail_title_for(machine: Option<&str>, name: &str) -> String {
    match machine {
        Some(m) => format!("{m} failed: {name}"),
        None => format!("failed: {name}"),
    }
}

fn parse_delete_report(text: &str) -> Option<DeleteReport> {
    let trimmed = text.trim();
    if let Ok(report) = serde_json::from_str::<DeleteReport>(trimmed) {
        return Some(report);
    }
    let start = trimmed.find('{')?;
    let end = trimmed.rfind('}')?;
    serde_json::from_str::<DeleteReport>(&trimmed[start..=end]).ok()
}

pub(crate) fn format_delete_confirm(r: &DeleteReport) -> String {
    let n = r.closed_terminals.len();
    let term_line = if r.closed_terminals.is_empty() {
        format!("Closes {n} terminal(s) (roles and shells).")
    } else {
        format!(
            "Closes {n} terminal(s) (roles and shells: {}).",
            r.closed_terminals.join(", ")
        )
    };
    let wt_count = r.removed_worktrees.len() + r.pruned_worktrees.len();
    let br_count = r.deleted_branches.len();
    let mut prune_line = format!(
        "Removes the studio's worktrees ({wt_count}) and merged backlot/{}/* branches ({br_count}).",
        r.studio
    );
    if !r.kept.is_empty() {
        prune_line.push_str(&format!(" Kept unless --force: {}.", r.kept.join("; ")));
    }
    let archive_line = format!(
        "Archives the ledger to {}; it can be restored by hand.",
        r.archive
    );
    format!("{term_line}\n{prune_line}\n{archive_line}\nYour repository is not touched.")
}
