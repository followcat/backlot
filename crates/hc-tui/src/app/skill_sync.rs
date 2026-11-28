//! Agent Skill synchronization: the machine notice, its per-machine
//! dismissal memory, and the local/remote install-uninstall runner.

use std::collections::BTreeMap;
use std::path::PathBuf;

use backlot_model::req_wire::ReqAction;
use serde::{Deserialize, Serialize};

use super::App;

#[derive(Debug, Clone)]
pub(crate) struct SkillJob {
    pub machine: Option<String>,
    pub agents: Vec<String>,
    pub uninstall: bool,
    pub force: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct SkillNeed {
    pub machine: Option<String>,
    pub label: String,
    pub agents: Vec<String>,
    pub version: String,
}

#[derive(Debug)]
pub(crate) enum SkillEvent {
    Done {
        machine: Option<String>,
        action: &'static str,
        out: Result<String, String>,
    },
}

#[derive(Default, Serialize, Deserialize)]
struct Dismissals {
    machines: BTreeMap<String, String>,
}

fn key(machine: Option<&str>) -> String {
    machine.unwrap_or("").to_owned()
}

pub(crate) fn needs(app: &App) -> Vec<SkillNeed> {
    app.panel_summaries()
        .iter()
        .filter_map(|ms| {
            let skill = ms.summary.agent_skill.as_ref()?;
            let agents = skill.installable_agents();
            (!agents.is_empty()).then(|| SkillNeed {
                machine: ms.machine.clone(),
                label: ms.machine.clone().unwrap_or_else(|| "Local".into()),
                agents,
                version: skill.version.clone(),
            })
        })
        .collect()
}

impl App {
    fn skill_dismissals_path(&self) -> Option<PathBuf> {
        self.skill_dismissals_path.clone().or_else(|| {
            self.update_state_path
                .as_ref()
                .and_then(|path| path.parent().map(|dir| dir.join("skill-dismissals.json")))
        })
    }

    fn dismissals(&mut self) -> BTreeMap<String, String> {
        if self.skill_dismissals.is_none() {
            let loaded = self
                .skill_dismissals_path()
                .and_then(|path| std::fs::read_to_string(path).ok())
                .and_then(|text| serde_json::from_str::<Dismissals>(&text).ok())
                .unwrap_or_default();
            self.skill_dismissals = Some(loaded.machines);
        }
        self.skill_dismissals.clone().unwrap_or_default()
    }

    fn visible_skill_needs(&mut self) -> Vec<SkillNeed> {
        let dismissed = self.dismissals();
        needs(self)
            .into_iter()
            .filter(|need| {
                dismissed.get(key(need.machine.as_deref()).as_str()) != Some(&need.version)
            })
            .collect()
    }

    /// Raise or retire the single aggregate Agent Skill notice after
    /// each publish. Nothing here installs anything; the explicit key
    /// handler does.
    pub(crate) fn sync_skill_notice(&mut self) {
        let visible = self.visible_skill_needs();
        if visible.is_empty() {
            if self.skill_notice.is_some() {
                self.skill_notice = None;
                self.notice = None;
            }
            return;
        }
        if self.skill_notice.as_ref() == Some(&visible) {
            return;
        }
        let missing = visible.iter().any(|need| need.agents.iter().any(|_| true));
        let _ = missing;
        let mut parts = Vec::new();
        for need in &visible {
            parts.push(format!("{} ({})", need.label, need.agents.join(", ")));
        }
        let body = format!("on {}\n↵ install · d dismiss", parts.join(" · "));
        self.skill_notice = Some(visible);
        self.show_skill_notice("Agent skill missing or stale", &body);
    }

    /// Whether the notice owns plain input right now.
    pub(crate) fn skill_notice_keys_active(&self) -> bool {
        self.skill_notice.is_some()
            && !self.skill_busy
            && self.dialog.is_none()
            && self.menu.is_none()
            && self.settings.is_none()
            && self.navigator.is_none()
            && !self.help
            && self.popup.is_none()
    }

    pub(crate) fn skill_notice_install(&mut self) {
        let Some(needs) = self.skill_notice.clone() else {
            return;
        };
        self.skill_notice = None;
        for need in needs {
            self.start_skill_action(need.machine, need.agents, false, false);
        }
    }

    pub(crate) fn skill_notice_dismiss(&mut self) {
        let Some(needs) = self.skill_notice.take() else {
            return;
        };
        let mut dismissed = self.dismissals();
        for need in needs {
            dismissed.insert(key(need.machine.as_deref()), need.version);
        }
        self.skill_dismissals = Some(dismissed.clone());
        if let Some(path) = self.skill_dismissals_path()
            && let Some(parent) = path.parent()
            && std::fs::create_dir_all(parent).is_ok()
        {
            let doc = Dismissals {
                machines: dismissed,
            };
            if let Ok(text) = serde_json::to_string_pretty(&doc) {
                let _ = std::fs::write(path, format!("{text}\n"));
            }
        }
        self.notice = None;
    }

    /// Where skill dismissals persist — the update-check cache's directory.
    pub fn set_skill_dismissals_path(&mut self, path: Option<PathBuf>) {
        self.skill_dismissals_path = path;
    }

    /// Start one install/uninstall. Local runs in-process with the
    /// CLI's implementation; a machine document rides the stage's
    /// request queue.
    pub(crate) fn start_skill_action(
        &mut self,
        machine: Option<String>,
        agents: Vec<String>,
        uninstall: bool,
        force: bool,
    ) {
        let job = SkillJob {
            machine: machine.clone(),
            agents: agents.clone(),
            uninstall,
            force,
        };
        if self.skill_busy {
            self.skill_queue.push_back(job);
            return;
        }
        self.skill_busy = true;
        let tx = self.skill_tx.clone();
        let open = self.rpc_opener.clone();
        let endpoint = self.panel_endpoint.clone();
        let timeout = self.server_ack_timeout;
        let local_in_process = machine.is_none() && self.server_target.is_none();
        let action_name = if uninstall { "uninstall" } else { "install" };
        tokio::spawn(async move {
            let out = run_skill_action(
                open,
                endpoint,
                machine.clone(),
                agents,
                uninstall,
                force,
                timeout,
                local_in_process,
            )
            .await;
            let _ = tx
                .send(SkillEvent::Done {
                    machine,
                    action: action_name,
                    out,
                })
                .await;
        });
    }

    pub(crate) fn on_skill_event(&mut self, event: SkillEvent) {
        let SkillEvent::Done {
            machine,
            action,
            out,
        } = event;
        self.skill_busy = false;
        if let Some(job) = self.skill_queue.pop_front() {
            self.start_skill_action(job.machine, job.agents, job.uninstall, job.force);
        }
        let label = machine.unwrap_or_else(|| "Local".into());
        match out {
            Ok(text) => self.show_notice(
                "Agent skill",
                &format!(
                    "{action} on {label} finished\n{}",
                    text.lines().next().unwrap_or("done")
                ),
            ),
            Err(error) => self.show_notice(
                "Agent skill",
                &format!("{action} on {label} failed: {error}"),
            ),
        }
    }
}

#[allow(clippy::too_many_arguments)]
async fn run_skill_action(
    open: Option<hc_proto::watch::OpenRpc>,
    endpoint: Option<hc_proto::endpoint::Endpoint>,
    machine: Option<String>,
    agents: Vec<String>,
    uninstall: bool,
    force: bool,
    timeout: std::time::Duration,
    local_in_process: bool,
) -> Result<String, String> {
    if machine.is_none() && local_in_process {
        let env = backlot_stage::cli::Env::process();
        return if uninstall {
            backlot_stage::skill::uninstall_agents(&agents, &env)
        } else {
            backlot_stage::skill::install(&agents, false, force, &env)
        }
        .map_err(|e| e.message().to_owned());
    }
    let open = open.ok_or_else(|| "not connected to server".to_owned())?;
    let client = open().await.map_err(|e| e.to_string())?;
    let action = if uninstall {
        ReqAction::SkillUninstall {
            agents,
            as_role: None,
        }
    } else {
        ReqAction::SkillInstall {
            agents,
            force,
            as_role: None,
        }
    };
    let ack = backlot_stage::doc_client::send_request_on(
        &client,
        endpoint.as_ref(),
        machine.as_deref(),
        &action,
        timeout,
    )
    .await
    .map_err(|e| e.message().to_owned())?;
    if !ack.ok {
        return Err(ack.error.unwrap_or_else(|| "request refused".into()));
    }
    let result: backlot_model::req_results::SkillResult = serde_json::from_value(
        ack.result
            .ok_or_else(|| "missing skill result".to_owned())?,
    )
    .map_err(|e| e.to_string())?;
    Ok(result.output)
}
