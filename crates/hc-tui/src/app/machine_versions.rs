//! U6c's App wiring: per-machine `backlot` versions, the machine
//! row's menu, the install/upgrade popup and the stage restart
//! that follows a successful upgrade. The pure state machine lives
//! in `crate::machine_versions`; this file only feeds it the view
//! (`machine.list` + each machine's `backlot` document), spawns
//! probes through `terminal.run`, and draws the answers.

use std::collections::BTreeMap;
use std::time::Instant;

use crate::chrome::render::{MachineLine, MachineTone};
use crate::chrome::snapshot::ClientEndpointId;
use crate::machine_versions::Backlot;
use crate::operations::Action;
use crate::popup_size::{PopupSize, resolve_popup_geometry};
use crate::studio_menu::{MenuBacklot, StudioMenu};

use super::App;
use super::creation::program_env;
use super::popup::{POPUP_LABEL, PopupPending};

/// One probe reply — `--version` answers fill the cache, the
/// install flow's env probe opens the popup or the refusal.
pub(crate) enum MachineProbe {
    /// `backlot --version` on `machine` answered.
    Version {
        machine: String,
        out: Result<String, String>,
    },
    /// The install flow's read-only env probe answered.
    InstallEnv {
        machine: String,
        out: Result<String, String>,
    },
}

/// The menu/sidebar line for one machine — `backlot`'s own version
/// and tone, plus the endpoint's `hc` version when the inventory
/// knows it.
fn machine_line(hc: Option<&str>, backlot: &Backlot, latest: Option<&str>) -> MachineLine {
    let (text, tone) = match backlot {
        Backlot::Installed { version, .. } => match latest {
            Some(latest) if crate::machine_versions::behind(version, latest) => {
                (format!("↑{latest}"), MachineTone::Accent)
            }
            _ => (format!("backlot {version}"), MachineTone::Normal),
        },
        Backlot::Missing => ("no backlot".to_owned(), MachineTone::Dim),
        Backlot::Probing | Backlot::Unknown => ("backlot …".to_owned(), MachineTone::Dim),
        Backlot::Error(_) => ("backlot ?".to_owned(), MachineTone::Dim),
    };
    MachineLine {
        hc: hc.map(str::to_owned),
        text,
        tone,
    }
}

/// The first non-empty line of a probe failure, clipped.
fn short(text: &str) -> String {
    text.lines()
        .map(str::trim)
        .find(|l| !l.is_empty())
        .unwrap_or("command failed")
        .chars()
        .take(64)
        .collect()
}

impl App {
    /// A machine's current `backlot` fact: this core runs the
    /// binary it was built from, a remote's document answers first,
    /// then the probe cache — with the runner's located path when a
    /// probe or an earlier locate knows it.
    pub(super) fn machine_backlot(&self, machine: Option<&str>) -> Backlot {
        match machine {
            None => Backlot::Installed {
                version: env!("CARGO_PKG_VERSION").to_owned(),
                exe: Some(crate::live_exe::live_exe().to_string_lossy().into_owned()),
            },
            Some(machine) => {
                let summaries = self.panel_summaries();
                let doc = summaries
                    .iter()
                    .find(|ms| ms.machine.as_deref() == Some(machine))
                    .and_then(|ms| ms.summary.backlot_version.as_deref());
                let mut state = self.machine_versions.state(machine, doc);
                if let Backlot::Installed { exe, .. } = &mut state
                    && exe.is_none()
                {
                    *exe = self
                        .remote_runner
                        .as_ref()
                        .and_then(|r| r.located_path(machine));
                }
                state
            }
        }
    }

    /// The newest release the one `update_check` knows — `update_latest`
    /// (a newer-than-running release) or the checked tag itself, as a
    /// normalized `x.y.z`. `None` while no check ever ran — machines
    /// then show their version plain, with no `↑` hint.
    pub(super) fn latest_release(&self) -> Option<String> {
        [
            self.update_latest.as_deref(),
            self.update_state.tag.as_deref(),
        ]
        .into_iter()
        .flatten()
        .filter_map(|v| {
            crate::update_check::semver(v).map(|(a, b, c)| ((a, b, c), format!("{a}.{b}.{c}")))
        })
        .max_by_key(|(t, _)| *t)
        .map(|(_, v)| v)
    }

    /// The endpoint→version-line map the expanded sidebar draws —
    /// `None` in plain mode, so Herdr's machines tree never sees it.
    pub(crate) fn machine_lines(&self) -> BTreeMap<ClientEndpointId, MachineLine> {
        let latest = self.latest_release();
        let mut out = BTreeMap::new();
        for endpoint in &self.projection.endpoints {
            let line = match &endpoint.endpoint_id {
                ClientEndpointId::Local => {
                    machine_line(None, &self.machine_backlot(None), latest.as_deref())
                }
                ClientEndpointId::Remote(name) => {
                    let hc = self
                        .view
                        .machines
                        .as_deref()
                        .unwrap_or_default()
                        .iter()
                        .find(|m| m.name == *name)
                        .map(|m| m.version.as_str());
                    machine_line(hc, &self.machine_backlot(Some(name)), latest.as_deref())
                }
            };
            out.insert(endpoint.endpoint_id.clone(), line);
        }
        out
    }

    /// The endpoint row's U6c menu — install/upgrade, the stage and
    /// `New studio here` above the endpoint ops it had already.
    pub(super) fn machine_menu(&self, id: &ClientEndpointId) -> StudioMenu {
        let machine = match id {
            ClientEndpointId::Local => None,
            ClientEndpointId::Remote(name) => Some(name.clone()),
        };
        let backlot = match self.machine_backlot(machine.as_deref()) {
            Backlot::Installed { version, .. } => MenuBacklot::Installed(version),
            Backlot::Missing => MenuBacklot::Missing,
            _ => MenuBacklot::Unknown,
        };
        let finished = self.machine_finished_ids(machine.as_deref()).len();
        let open = self.studio.finished_open.contains(&machine);
        StudioMenu::machine_row(
            machine.clone(),
            backlot,
            !self.machine_stage_running(machine.as_deref()),
            !self.machine_reachable(machine.as_deref()),
            self.latest_release(),
        )
        .with_finished(finished, open)
    }

    // ---------- probing ----------

    /// Called after every publish: `machine.list` rows drive the
    /// probe cache — a machine's own document answers for it, the
    /// rest probe at most once per `PROBE_EVERY`. A machine whose
    /// install popup closed re-probes at once (its doc's version is
    /// the old stage's word until the restart publishes the new).
    pub(super) fn sync_machine_versions(&mut self) {
        if self.plain {
            return;
        }
        let now = Instant::now();
        // `<machine> → doc version` — the install flow's in-flight
        // machines skip it so their post-install probe still runs.
        let docs: BTreeMap<String, String> = self
            .panel_summaries()
            .iter()
            .filter_map(|ms| {
                ms.machine
                    .as_deref()
                    .zip(ms.summary.backlot_version.as_deref())
                    .map(|(m, v)| (m.to_owned(), v.to_owned()))
            })
            .collect();
        let rows: Vec<crate::machine_versions::Row<'_>> = self
            .view
            .machines
            .as_deref()
            .unwrap_or_default()
            .iter()
            .map(|m| crate::machine_versions::Row {
                name: &m.name,
                online: m.state == "connected",
                doc_version: docs
                    .get(&m.name)
                    .filter(|_| !self.install_upgrade.contains_key(&m.name))
                    .map(String::as_str),
            })
            .collect();
        for machine in self.machine_versions.want_probes(now, &rows) {
            self.machine_versions.note_probe_start(&machine, now);
            let run = self.ns_runner();
            let tx = self.mv_tx.clone();
            tokio::spawn(async move {
                let out = run(Some(machine.clone()), vec!["--version".into()], None).await;
                let _ = tx.send(MachineProbe::Version { machine, out }).await;
            });
        }
    }

    /// A machine probe reply landed.
    pub(crate) fn on_mv_reply(&mut self, reply: MachineProbe) {
        match reply {
            MachineProbe::Version { machine, out } => {
                self.machine_versions.note_reply(&machine, out);
                // An install popup's re-probe just landed: a bumped
                // version on a machine whose stage ran restarts it —
                // the ledger replays, nothing is lost (U6c §5). The
                // probe's own verdict is the comparison — the
                // document's `backlot_version` is the still-running
                // old stage's word until the restart republishes.
                if let Some(old) = self.install_upgrade.remove(&machine)
                    && let Backlot::Installed { version, .. } =
                        self.machine_versions.state(&machine, None)
                    && Some(&version) != old.as_ref()
                    && self.machine_stage_running(Some(&machine))
                {
                    self.machine_stage_restart(Some(machine));
                }
            }
            MachineProbe::InstallEnv { machine, out } => {
                self.install_env.remove(&machine);
                match out {
                    Ok(text) => self.machine_install_classified(&machine, &text),
                    Err(e) => {
                        self.show_notice("Install backlot", &format!("{machine}: {}", short(&e)))
                    }
                }
            }
        }
        self.sched.mark_dirty(Instant::now());
    }

    // ---------- install / upgrade ----------

    /// The machine menu's install/upgrade: this core defers to the
    /// update card's own managed-binary refusal and flow; a remote
    /// machine first gets the read-only env probe so its binary's
    /// channel classifies before any installer runs (W2).
    pub(super) fn machine_install(&mut self, machine: Option<String>) {
        match machine {
            None => {
                if let Some(refusal) = self.install_source.refusal() {
                    self.show_notice("Upgrade backlot", &refusal);
                    return;
                }
                self.open_update();
            }
            Some(machine) => {
                if !self.machine_reachable(Some(&machine)) {
                    self.show_notice("Install backlot", &format!("{machine} is unreachable"));
                    return;
                }
                let Some(runner) = self.remote_runner.clone() else {
                    self.show_notice(
                        "Install backlot",
                        &format!("cannot reach {machine}: no remote connection"),
                    );
                    return;
                };
                if !self.install_env.insert(machine.clone()) {
                    return; // an env probe is already in flight
                }
                let tx = self.mv_tx.clone();
                tokio::spawn(async move {
                    let out = runner
                        .run_script(machine.clone(), crate::machine_install::env_probe_script())
                        .await;
                    let _ = tx.send(MachineProbe::InstallEnv { machine, out }).await;
                });
            }
        }
    }

    /// The env probe's reply: classify the remote binary's install
    /// channel — a managed one refuses with its own upgrade command
    /// (never an `install.sh` over a package manager's file) — then
    /// pin `BACKLOT_VERSION` and open the installer popup.
    fn machine_install_classified(&mut self, machine: &str, text: &str) {
        let (mut exe, env) = crate::machine_install::parse_env_probe(text);
        if exe.is_none() {
            // The probe printed no `EXE=` — the runner's locate cache
            // may still know the binary the last call found.
            exe = self
                .remote_runner
                .as_ref()
                .and_then(|r| r.located_path(machine));
        }
        let env = crate::install_source::SourceEnv::from_lookup(|key| {
            env.get(key).map(std::ffi::OsString::from)
        });
        let source = exe
            .as_deref()
            .map_or(crate::install_source::InstallSource::Direct, |exe| {
                crate::install_source::detect(std::path::Path::new(exe), &env)
            });
        if let Some(refusal) = source.refusal() {
            self.show_notice("Upgrade backlot", &format!("{machine}: {refusal}"));
            return;
        }
        // `Upgrade` always has the known latest; a fresh `Install`
        // pins the version this client runs when no release is known.
        let version = self
            .latest_release()
            .unwrap_or_else(|| env!("CARGO_PKG_VERSION").to_owned());
        let dir = crate::machine_install::install_dir(exe.as_deref());
        let line = match crate::machine_install::install_line(&version, dir.as_deref()) {
            Ok(line) => line,
            Err(e) => {
                self.show_notice("Install backlot", &e);
                return;
            }
        };
        self.spawn_install_popup(machine, line);
    }

    /// The installer popup on the target machine: `terminal.run
    /// on: <machine>` shows the `curl | sh` pipeline's own output —
    /// the run's `rm` reaps the terminal when `sh` exits, and the
    /// popup's close then re-probes the machine's version.
    fn spawn_install_popup(&mut self, machine: &str, line: String) {
        if self.popup.is_some() || self.popup_pending.is_some() {
            self.dismiss_popup();
        }
        let geometry = resolve_popup_geometry(
            Some(PopupSize::Percent(80)),
            Some(PopupSize::Percent(60)),
            self.frame_area(),
        );
        let mut params = hc_proto::terminal::RunParams {
            line: Some(line),
            on: Some(machine.to_owned()),
            rm: true,
            keep: false,
            no_notify: true,
            no_parent: true,
            env: program_env(),
            labels: BTreeMap::from([(POPUP_LABEL.into(), "1".into())]),
            display: self.host_facts.display(),
            ..Default::default()
        };
        if let Some(geometry) = geometry {
            params.size = Some(hc_proto::terminal::InitialSize {
                cols: geometry.terminal_cols,
                rows: geometry.terminal_rows,
            });
        }
        // A bumped version on a stage-running machine chains the
        // restart once the re-probe answers — the pre-install version
        // is the comparison.
        let old = match self.machine_backlot(Some(machine)) {
            Backlot::Installed { version, .. } => Some(version),
            _ => None,
        };
        if self.machine_stage_running(Some(machine)) {
            self.install_upgrade.insert(machine.to_owned(), old);
        }
        self.popup_pending = Some(PopupPending {
            width: Some(PopupSize::Percent(80)),
            height: Some(PopupSize::Percent(60)),
            persistent: false,
            owner: String::new(),
            command: String::new(),
            install_machine: Some(machine.to_owned()),
        });
        self.submit_action(Action::RunPopup { params });
    }

    /// The install popup closed — the machine's locate cache and
    /// verdict are stale now, so both drop and the next sync
    /// re-probes `backlot --version` on it at once.
    pub(super) fn machine_install_closed(&mut self, machine: &str) {
        if let Some(runner) = &self.remote_runner {
            runner.forget(machine);
        }
        self.machine_versions.invalidate(machine);
        self.sync_machine_versions();
        self.sched.mark_dirty(Instant::now());
    }

    // ---------- stage restart ----------

    /// `backlot stage stop && stage start` on the machine — the
    /// restart marker lands on the stop action, and its `Ok` reply
    /// chains the start through the same ActionTracker (U6c §5).
    pub(super) fn machine_stage_restart(&mut self, machine: Option<String>) {
        self.stage_restart_pending.insert(machine.clone());
        self.run_action(crate::studio_cmds::stage_stop(machine));
    }

    /// A `stop stage` action's reply: `Ok` and still-pending chains
    /// `stage start` on the same machine. Called from
    /// `on_action_reply` before the toast branch.
    pub(super) fn machine_stage_restart_chain(&mut self, action: &crate::action::Action, ok: bool) {
        if action.name != "stop stage" {
            return;
        }
        if self.stage_restart_pending.remove(&action.machine) && ok {
            self.run_action(crate::studio_cmds::stage_start(action.machine.clone()));
        }
    }
}
