//! The New Studio dialog's App wiring (S4): every entry point lands
//! on `new_studio_open`, the dialog owns keys and mouse while it is
//! open, and each `backlot` call — `current_exe` locally, a
//! `terminal.run on:` through `RemoteRunner` on an added machine —
//! answers on `ns_rx` tagged by generation, so a stale reply never
//! lands on a switched machine.

mod keys;

use std::collections::BTreeMap;
use std::sync::Arc;

use super::{App, BoxFut};
use crate::chrome::snapshot::ClientEndpointId;
use crate::new_studio::{self, CastTable, MachineRow, MachineState, Probe, Step1Pane};
use crate::operations::Action;
use crate::studio::StudioKey;

/// One async answer the dialog waits on. `epoch` matches the state's
/// generation — a machine switch or a `^R` re-check invalidates
/// in-flight probes so a slow machine cannot overwrite a newer row.
pub(crate) enum NsReply {
    Version {
        epoch: u64,
        machine: Option<String>,
        out: Result<String, String>,
    },
    Repos {
        epoch: u64,
        out: Result<String, String>,
    },
    Runners {
        epoch: u64,
        out: Result<String, String>,
    },
    /// The Hollywood setup dialog's own `runners --json` — the same
    /// probe with its own generation tag.
    HwRunners {
        epoch: u64,
        out: Result<String, String>,
    },
    Dry {
        epoch: u64,
        out: Result<String, String>,
    },
    Create {
        epoch: u64,
        machine: Option<String>,
        id: String,
        /// `studio new` itself answered Ok — the studio exists even
        /// when a following `stage start` failed the pair, so the
        /// cast it used is the machine's new last-used cast either
        /// way.
        created: bool,
        out: Result<String, String>,
    },
}

/// The runner call the dialog's spawns share: machine (`None` is
/// this core), argv after `backlot`, optional stdin for `--cast -`.
pub(crate) type NsRun = Arc<
    dyn Fn(Option<String>, Vec<String>, Option<String>) -> BoxFut<Result<String, String>>
        + Send
        + Sync,
>;

impl App {
    /// Every entry point (`Ctrl+B N` over a studio, `n` on the page
    /// or in the Call Sheet, `+ new studio`, the sidebar's
    /// `new · <machine>`) opens the same dialog; `machine` is the
    /// sidebar's preselected target or the current studio's.
    pub(crate) fn new_studio_open(&mut self, machine: Option<String>) {
        if self.plain {
            return;
        }
        self.release_pane_mouse();
        self.chrome_drag = None;
        let mut d = new_studio::NewStudio::new();
        d.machines = self.ns_machine_rows();
        if let Some(name) = &machine
            && let Some(i) = d
                .machines
                .iter()
                .position(|row| row.name.as_deref() == Some(name.as_str()))
        {
            d.machine_sel = i;
            // A caller that named a machine wants its repo list —
            // start on the right-hand pane.
            d.pane = Step1Pane::Repos;
        }
        self.new_studio = Some(d);
        self.ns_probe_versions();
        self.ns_refresh_studios();
        self.ns_probe_repos(None);
    }

    /// `Local` plus each attached machine with its reachability;
    /// the backlot probe fills the version in behind.
    fn ns_machine_rows(&self) -> Vec<MachineRow> {
        let mut rows = vec![MachineRow {
            name: None,
            label: "Local".into(),
            state: MachineState::Online,
            backlot: Probe::Pending,
        }];
        for endpoint in &self.projection.endpoints {
            let ClientEndpointId::Remote(name) = &endpoint.endpoint_id else {
                continue;
            };
            use crate::chrome::snapshot::ClientEndpointStatus as S;
            let state = match endpoint.status {
                S::Online | S::Attention => MachineState::Online,
                S::Connecting | S::Reconnecting => MachineState::Offline("reconnecting…".into()),
                S::Disabled => MachineState::Offline("disabled".into()),
            };
            let online = state == MachineState::Online;
            rows.push(MachineRow {
                name: Some(name.clone()),
                label: endpoint.label.clone(),
                state,
                backlot: if online {
                    Probe::Pending
                } else {
                    Probe::Failed("offline".into())
                },
            });
        }
        rows
    }

    /// The `backlot` call for a machine: `live_exe()` locally with
    /// the endpoint flags, `RemoteRunner::run_output` remotely —
    /// never a remote path checked against the local disk (A6.3).
    /// Tests inject their own through `ns_run`. The sheet's ledger
    /// reads and `o` report streams ride the same runner (S2 §3).
    pub(crate) fn ns_runner(&self) -> NsRun {
        if let Some(run) = &self.ns_run {
            return run.clone();
        }
        let remote = self.remote_runner.clone();
        let endpoint_args = self.panel_endpoint_args.clone();
        Arc::new(move |machine, argv, stdin| {
            let remote = remote.clone();
            match machine {
                Some(machine) => match remote {
                    Some(runner) => runner.run_output(machine, argv, stdin),
                    None => Box::pin(async move {
                        Err(format!("cannot reach {machine}: no remote connection"))
                    }),
                },
                None => {
                    // Endpoint flags ride after the subcommand — the
                    // same argv shape `panel_argv` builds.
                    let mut full = argv.clone();
                    full.extend(endpoint_args.iter().cloned());
                    let exe = crate::live_exe::live_exe();
                    Box::pin(run_local(exe, full, stdin))
                }
            }
        })
    }

    /// `backlot --version` on every online machine — a remote probe
    /// answers `not installed` with the same text the panel's
    /// actions report.
    fn ns_probe_versions(&mut self) {
        let run = self.ns_runner();
        let tx = self.ns_tx.clone();
        let Some(d) = &mut self.new_studio else {
            return;
        };
        d.machines_gen += 1;
        let epoch = d.machines_gen;
        d.refreshing = true;
        for row in &mut d.machines {
            if row.state == MachineState::Online {
                row.backlot = Probe::Pending;
            }
        }
        for row in d.machines.clone() {
            if row.state != MachineState::Online {
                continue;
            }
            let run = run.clone();
            let tx = tx.clone();
            let machine = row.name;
            tokio::spawn(async move {
                let out = run(machine.clone(), vec!["--version".into()], None)
                    .await
                    .map(|text| {
                        text.lines()
                            .map(str::trim)
                            .find(|l| !l.is_empty())
                            .unwrap_or("")
                            .trim_start_matches("backlot ")
                            .to_string()
                    });
                let _ = tx
                    .send(NsReply::Version {
                        epoch,
                        machine,
                        out,
                    })
                    .await;
            });
        }
    }

    /// `repos --json` on the selected machine — `None` asks for its
    /// `$HOME`, `Some(dir)` a directory it named (a descend or an
    /// ascend the breadcrumb allowed).
    fn ns_probe_repos(&mut self, dir: Option<String>) {
        let run = self.ns_runner();
        let tx = self.ns_tx.clone();
        let Some(d) = &mut self.new_studio else {
            return;
        };
        d.repos.epoch += 1;
        let epoch = d.repos.epoch;
        d.repos.listing = Probe::Pending;
        d.repos.sel = 0;
        d.repos.scroll = 0;
        // A descend/ascend replaces the listing — a filter written
        // for the old one would hide rows in the new.
        if dir.is_some() {
            d.repos.filter.clear();
        }
        let machine = d.machine_name();
        let mut argv = vec!["repos".into(), "--json".into()];
        if let Some(dir) = dir {
            argv.extend(["--from".into(), dir]);
        }
        tokio::spawn(async move {
            let out = run(machine, argv, None).await;
            let _ = tx.send(NsReply::Repos { epoch, out }).await;
        });
    }

    /// `runners --json` on the selected machine — step 3's probe.
    fn ns_probe_runners(&mut self) {
        let run = self.ns_runner();
        let tx = self.ns_tx.clone();
        let Some(d) = &mut self.new_studio else {
            return;
        };
        d.runners_gen += 1;
        let epoch = d.runners_gen;
        d.runners = Probe::Pending;
        let machine = d.machine_name();
        tokio::spawn(async move {
            let out = run(machine, vec!["runners".into(), "--json".into()], None).await;
            let _ = tx.send(NsReply::Runners { epoch, out }).await;
        });
    }

    /// The studio ids the dup check works against — the selected
    /// machine's own list from the published summaries (A6.2).
    fn ns_refresh_studios(&mut self) {
        let Some(d) = &self.new_studio else { return };
        let machine = d.machine_name();
        let studios: Vec<String> = self
            .panel_summaries()
            .into_iter()
            .filter(|m| m.machine == machine)
            .flat_map(|m| m.summary.studios.into_iter().map(|s| s.id))
            .collect();
        if let Some(d) = &mut self.new_studio {
            d.studios = studios;
        }
    }

    /// The `studio new` argv — shared by the dry run and the
    /// create; `--cast -` takes the table's toml on stdin. A plain
    /// directory rides `--init` or the step-4 toggle's `--no-git`
    /// (D4); repositories and workspaces take neither.
    fn ns_studio_argv(&self, dry_run: bool) -> Option<(Option<String>, Vec<String>, String)> {
        let d = self.new_studio.as_ref()?;
        let entry = d.repo()?;
        let mut argv = vec![
            "studio".into(),
            "new".into(),
            "--studio".into(),
            d.id.as_str().to_string(),
            "--title".into(),
            d.title.as_str().to_string(),
            "--repo".into(),
            entry.path.clone(),
            "--cast".into(),
            "-".into(),
        ];
        if entry.shape() == new_studio::PathShape::Plain {
            argv.push(if d.no_git { "--no-git" } else { "--init" }.into());
        }
        if dry_run {
            argv.push("--dry-run".into());
        }
        let runners = match &d.runners {
            Probe::Done(r) => r.as_slice(),
            _ => &[],
        };
        Some((d.machine_name(), argv, d.cast.toml(runners, d.mode)))
    }

    /// Step 4's dry run — `studio new … --dry-run` on the target
    /// machine; its verdict (or the exact error) paints the step.
    fn ns_dry_run(&mut self) {
        let Some((machine, argv, cast)) = self.ns_studio_argv(true) else {
            return;
        };
        let run = self.ns_runner();
        let tx = self.ns_tx.clone();
        let Some(d) = &mut self.new_studio else {
            return;
        };
        d.dry_gen += 1;
        let epoch = d.dry_gen;
        d.dry = Probe::Pending;
        tokio::spawn(async move {
            let out = run(machine, argv, Some(cast)).await;
            let _ = tx.send(NsReply::Dry { epoch, out }).await;
        });
    }

    /// `studio new` created on `machine` — the cast it used becomes
    /// that machine's step-3 prefill, persisted in the endpoint's
    /// preferences file. The rows store names, not indexes, so a
    /// runner list that shifted by the next open still resolves.
    fn ns_save_cast(&mut self, machine: &Option<String>) {
        let Some(d) = &self.new_studio else {
            return;
        };
        let Probe::Done(runners) = &d.runners else {
            return;
        };
        let rows = d
            .cast
            .rows
            .iter()
            .filter_map(|row| {
                let runner = runners.get(row.runner)?;
                Some(crate::preferences::SavedCastRow {
                    key: row.key.clone(),
                    candidate: row.candidate,
                    runner: runner.runner.clone(),
                    model: runner.models.get(row.model).cloned(),
                    effort: runner.efforts.get(row.effort).cloned(),
                })
            })
            .collect();
        let saved = crate::preferences::SavedStudioCast {
            machine: machine.clone(),
            rows,
            custom: d.cast.custom.clone(),
        };
        self.ns_saved_casts.retain(|c| &c.machine != machine);
        self.ns_saved_casts.push(saved);
        self.persist_chrome_preferences();
    }

    /// `Enter` on step 4: `studio new` on the target machine, then
    /// `stage start` there when the box is checked. The studio's
    /// publish lands the focus through `ns_pending_focus`.
    fn ns_create(&mut self) {
        if self.new_studio.as_ref().is_none_or(|d| d.creating) {
            return;
        }
        let Some((machine, argv, cast)) = self.ns_studio_argv(false) else {
            return;
        };
        let run = self.ns_runner();
        let tx = self.ns_tx.clone();
        let Some(d) = &mut self.new_studio else {
            return;
        };
        let start_stage = d.start_stage;
        let id = d.id.as_str().to_string();
        d.creating = true;
        d.create_gen += 1;
        let epoch = d.create_gen;
        d.create_error = None;
        let stage_machine = machine.clone();
        tokio::spawn(async move {
            let first = run(machine.clone(), argv, Some(cast)).await;
            let created = first.is_ok();
            let out = match first {
                Ok(text) => {
                    if start_stage {
                        run(stage_machine, vec!["stage".into(), "start".into()], None)
                            .await
                            .map(|tail| format!("{text}\n{tail}"))
                    } else {
                        Ok(text)
                    }
                }
                Err(e) => Err(e),
            };
            let _ = tx
                .send(NsReply::Create {
                    epoch,
                    machine,
                    id,
                    created,
                    out,
                })
                .await;
        });
    }

    /// The studio exists: the wizard has done its job and goes, with
    /// the focus owed to the studio's producer until its publish
    /// seats one (`ns_focus_pending`).
    fn ns_created(&mut self, machine: Option<String>, id: String) {
        self.new_studio = None;
        self.ns_pending_focus = Some(StudioKey { machine, id });
    }

    /// A reply off `ns_rx` — land it only when its generation and
    /// dialog still match the request.
    pub(crate) fn on_ns_reply(&mut self, reply: NsReply) {
        match reply {
            NsReply::Version {
                epoch,
                machine,
                out,
            } => {
                let Some(d) = &mut self.new_studio else {
                    return;
                };
                if epoch != d.machines_gen {
                    return;
                }
                if let Some(row) = d.machines.iter_mut().find(|row| row.name == machine) {
                    row.backlot = match out {
                        Ok(v) => Probe::Done(v),
                        Err(e) => Probe::Failed(e),
                    };
                }
                d.refreshing = d
                    .machines
                    .iter()
                    .any(|row| row.state == MachineState::Online && row.backlot.is_pending());
            }
            NsReply::Repos { epoch, out } => {
                let Some(d) = &mut self.new_studio else {
                    return;
                };
                if epoch != d.repos.epoch {
                    return;
                }
                match out {
                    Ok(text) => match new_studio::parse_repos(&text) {
                        Ok((cwd, entries)) => {
                            if d.repos.dir.is_none() {
                                d.repos.home = Some(cwd.clone());
                            }
                            d.repos.dir = Some(cwd);
                            d.repos.listing = Probe::Done(entries);
                        }
                        Err(e) => d.repos.listing = Probe::Failed(e),
                    },
                    Err(e) => d.repos.listing = Probe::Failed(e),
                }
            }
            NsReply::Runners { epoch, out } => {
                let Some(d) = &mut self.new_studio else {
                    return;
                };
                if epoch != d.runners_gen {
                    return;
                }
                d.runners = match out {
                    Ok(text) => match new_studio::parse_runners(&text) {
                        Ok(rows) => {
                            // Draft the table once — a `^R` refresh
                            // reseats nothing the user arranged. The
                            // machine's last created cast prefills
                            // the rows when one was saved.
                            if d.cast.rows.is_empty() {
                                d.cast = self
                                    .ns_saved_casts
                                    .iter()
                                    .find(|c| c.machine == d.machine_name())
                                    .map(|saved| CastTable::from_saved(&rows, saved))
                                    .unwrap_or_else(|| CastTable::draft(&rows));
                            }
                            Probe::Done(rows)
                        }
                        Err(e) => Probe::Failed(e),
                    },
                    Err(e) => Probe::Failed(e),
                };
            }
            NsReply::HwRunners { epoch, out } => self.hw_runners_reply(epoch, out),
            NsReply::Dry { epoch, out } => {
                let Some(d) = &mut self.new_studio else {
                    return;
                };
                if epoch != d.dry_gen {
                    return;
                }
                d.dry = match out {
                    Ok(text) => Probe::Done(text),
                    Err(e) => Probe::Failed(e),
                };
            }
            NsReply::Create {
                epoch,
                machine,
                id,
                created,
                out,
            } => {
                let Some(d) = &mut self.new_studio else {
                    return;
                };
                if epoch != d.create_gen {
                    return;
                }
                d.creating = false;
                // The studio exists — the cast it used is this
                // machine's step-3 prefill from now on. Recorded
                // before the dialog can drop, never on a dry run.
                if created {
                    self.ns_save_cast(&machine);
                }
                match out {
                    // A created studio replaces its wizard.
                    Ok(_) => self.ns_created(machine, id),
                    // The studio exists even when a later call in
                    // the pair failed — `created` is `studio new`'s
                    // own verdict. The create asked for a studio, and
                    // it got one: close the wizard and let the
                    // failure be a notice, never a dialog the person
                    // has to Esc out of a studio they already have.
                    Err(e) if created => {
                        self.show_notice("Studio created", &e);
                        self.ns_created(machine, id);
                    }
                    // A refused `studio new` keeps the draft: the
                    // person is still editing and will retry.
                    Err(e) => {
                        if let Some(d) = &mut self.new_studio {
                            d.create_error = Some(e);
                        }
                    }
                }
            }
        }
    }

    /// IME/commit text and pastes while the dialog is open land in
    /// the live editor — a model cell's `e` or open list, step 1's
    /// filter, or step 2's focused field — never in the terminal
    /// behind it (the dialog owns input, like `insert_dialog_text`'s
    /// rename).
    pub(super) fn insert_ns_text(&mut self, text: &str) -> bool {
        let Some(d) = &mut self.new_studio else {
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
        if d.step == new_studio::Step::MachineRepo {
            if d.repos.filter.insert(text) {
                d.pane = new_studio::Step1Pane::Repos;
                d.repos.sel = 0;
                d.repos.scroll = 0;
            }
            return true;
        }
        if d.step == new_studio::Step::IdTitle {
            match d.field {
                new_studio::Step2Field::Id => {
                    d.id.insert(text);
                }
                new_studio::Step2Field::Title => {
                    d.title.insert(text);
                }
                new_studio::Step2Field::Mode => {}
            }
        }
        true
    }

    /// `^E`'s editor popup: `$VISUAL`/`$EDITOR <file>` in a floating
    /// terminal on this host — the cast file is always client-local
    /// (it travels to the target machine over stdin later), so `on`
    /// is never set even when a remote terminal has focus.
    fn spawn_ns_editor(&mut self, line: String) {
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
            labels: BTreeMap::from([(super::popup::POPUP_LABEL.into(), "1".into())]),
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
        self.submit_action(Action::RunPopup { params });
    }

    /// Tests inject a recorder; production leaves `ns_run` at
    /// `None` and the runner above does the real local/remote call.
    #[cfg(test)]
    pub(crate) fn set_ns_run(&mut self, run: NsRun) {
        self.ns_run = Some(run);
    }

    /// The created studio's document landed — focus its producer,
    /// the same address `focus_studio` resolves for a panel Enter.
    /// The stage publishes a role `live` once its terminal seats, so
    /// a summary that names the studio but seats nobody keeps the
    /// pending focus for the next publish.
    pub(crate) fn ns_focus_pending(&mut self) {
        let Some(key) = self.ns_pending_focus.clone() else {
            return;
        };
        let machines = self.panel_summaries();
        let Some(studio) = crate::panel::data::studio(&machines, &key) else {
            return;
        };
        let seated = crate::studio::cast_order(studio.studio)
            .iter()
            .any(|role| crate::studio::live_terminal(role).is_some());
        if seated {
            self.ns_pending_focus = None;
            self.focus_studio(&key);
        }
    }
}

/// `backlot <argv>` on this core — the executable is this one, the
/// endpoint flags ride in `argv`. The exit text on a failure is the
/// combined output, verbatim: a dry-run error shows its own words.
async fn run_local(
    exe: std::path::PathBuf,
    argv: Vec<String>,
    stdin: Option<String>,
) -> Result<String, String> {
    let mut cmd = tokio::process::Command::new(&exe);
    cmd.args(&argv)
        .stdin(if stdin.is_some() {
            std::process::Stdio::piped()
        } else {
            std::process::Stdio::null()
        })
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped());
    let mut child = cmd
        .spawn()
        .map_err(|e| format!("cannot run backlot: {e}"))?;
    if let Some(text) = stdin {
        use tokio::io::AsyncWriteExt;
        if let Some(mut pipe) = child.stdin.take() {
            let _ = pipe.write_all(text.as_bytes()).await;
            let _ = pipe.shutdown().await;
        }
    }
    let out = child
        .wait_with_output()
        .await
        .map_err(|e| format!("cannot run backlot: {e}"))?;
    let stdout = String::from_utf8_lossy(&out.stdout);
    let stderr = String::from_utf8_lossy(&out.stderr);
    if out.status.success() {
        Ok(stdout.into_owned())
    } else {
        // Verbatim — stderr first like a terminal shows it, then
        // stdout when the command also printed there.
        let mut text = stderr.into_owned();
        if !stdout.trim().is_empty() {
            if !text.trim().is_empty() {
                text.push('\n');
            }
            text.push_str(&stdout);
        }
        if text.trim().is_empty() {
            text = "backlot command failed".into();
        }
        Err(text)
    }
}
