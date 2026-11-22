//! Studio shells (U2): in studio mode `new_tab` and the splits create
//! a shell that belongs to the current studio — the user's shell in a
//! workarea, never a stage role and never touched by the stage. The
//! label scheme (`backlot.studio`, `backlot.shell`, `herdr.space`,
//! `herdr.tab`) is what the cast's `shells` group, the per-studio
//! space and the stage's keep-away all read. With no studio the
//! actions land on the opening page instead.

use std::collections::BTreeMap;

use super::App;
use super::creation::program_env;
use crate::operations::{Action, Change, Creation, Edit};
use crate::shells::{
    SHELL_LABEL, STUDIO_LABEL, next_shell_n, shell_name, shell_tab_id, studio_shells,
};
use crate::studio::StudioKey;

impl App {
    /// `new_tab` and the splits in studio mode: a studio shell
    /// (`vertical` `Some` splits the focused tab, `None` a new
    /// `shell <n>` tab). Outside studio mode nothing changes — the
    /// caller keeps herdr's own `creation`.
    pub(super) fn studio_shell(&self, vertical: Option<bool>) -> Result<Creation, String> {
        let machines = self.panel_summaries();
        let index = self.studio.current_index(&machines).ok_or("No studio")?;
        let studios = crate::panel::data::studios(&machines);
        let current = studios[index];
        let studio = current.studio;
        let n = next_shell_n(&self.view, current.machine, &studio.id);
        let focused = self
            .focused
            .as_deref()
            .and_then(|id| self.view.inventory.get(id));

        // The studio's own machine — a remote studio keeps its
        // shells remote, never pulled local by a local focus.
        let machine = current.machine.map(str::to_owned);

        // The workarea of the person focused — an actor's worktree,
        // the director's integration worktree — else the studio's
        // repository, else the focused terminal's own cwd.
        let cwd = self
            .focused
            .as_deref()
            .and_then(|f| {
                studio
                    .roles
                    .iter()
                    .find(|r| r.terminal.as_deref() == Some(f))
            })
            .and_then(|r| r.workdir.clone())
            .or_else(|| studio.repo.clone())
            .or_else(|| focused.map(|t| t.cwd.clone()));

        // Labels: a new tab is `shell <n>` in the studio's space; a
        // split joins the focused tab, carrying its space and tab
        // labels so a rebuild groups it where the user split it.
        let (space_label, tab_label) = match vertical {
            None => (studio.id.clone(), format!("shell {n}")),
            Some(_) => {
                let focused = focused.ok_or("No focused terminal")?;
                (
                    focused
                        .labels
                        .get("herdr.space")
                        .cloned()
                        .unwrap_or_else(|| studio.id.clone()),
                    focused
                        .labels
                        .get("herdr.tab")
                        .cloned()
                        .unwrap_or_else(|| format!("shell {n}")),
                )
            }
        };
        let (space_id, tab_id) = shell_tab_id(&space_label, &tab_label);

        let mut edit = match vertical {
            None => Edit {
                tab: crate::project::layout_key(machine.as_deref(), &tab_id),
                space: crate::project::layout_key(machine.as_deref(), &space_id),
                fallback: crate::structure::Tree::pane("pending-created-terminal"),
                incarnations: self
                    .view
                    .inventory
                    .terminals()
                    .iter()
                    .map(|(id, t)| (id.clone(), t.created_ms))
                    .collect(),
                change: Change::NewTab {
                    terminal: String::new(),
                    name: String::new(),
                },
            },
            Some(vertical) => self
                .layout_edit(Change::Split {
                    target: focused.expect("split checks focused").address.clone(),
                    terminal: String::new(),
                    vertical,
                    zoomed: false,
                })
                .ok_or("No active layout")?,
        };
        edit.space = crate::project::layout_key(machine.as_deref(), &space_id);
        if vertical.is_none() {
            edit.tab = crate::project::layout_key(machine.as_deref(), &tab_id);
        }

        // `order`: the studio space's existing tab ids, so the new
        // tab appends after them instead of reordering the space.
        let app = self
            .view
            .documents
            .get(&hc_proto::watch::DocKey {
                machine: machine.clone(),
                name: "herdr-app".into(),
            })
            .map(|d| d.value.clone())
            .unwrap_or_default();
        let terms: Vec<_> = self
            .view
            .inventory
            .terminals()
            .values()
            .filter(|t| t.address.split_once('/').map(|(m, _)| m) == machine.as_deref())
            .collect();
        let order = crate::structure::read(&terms, &app, machine.as_deref())
            .into_iter()
            .find(|s| s.id == space_id)
            .map(|s| s.tabs.iter().map(|t| t.id.clone()).collect())
            .unwrap_or_else(Vec::new);

        Ok(Creation {
            params: hc_proto::terminal::RunParams {
                on: machine,
                cwd,
                name: Some(shell_name(&studio.id, n)),
                no_parent: true,
                no_notify: true,
                keep: true,
                rm: true,
                env: program_env(),
                labels: BTreeMap::from([
                    (STUDIO_LABEL.into(), studio.id.clone()),
                    (SHELL_LABEL.into(), n.to_string()),
                    ("herdr.space".into(), space_label),
                    ("herdr.tab".into(), tab_label),
                    ("herdr.seen".into(), "0".into()),
                ]),
                display: self.host_facts.display(),
                ..Default::default()
            },
            edit,
            order,
            space_adoption: None,
            report_created: false,
            promotion: None,
        })
    }

    /// `new_tab` outside `--plain`: a studio shell in studio mode,
    /// the opening page without one — never a stray plain terminal.
    pub(super) fn new_tab_studio(&mut self) {
        if self.studioless() {
            self.opening_show();
            return;
        }
        match self.studio_shell(None) {
            Ok(creation) => self.submit_action(Action::Create(Box::new(creation))),
            Err(error) => self.show_notice("New tab unavailable", &error),
        }
    }

    /// A split outside `--plain`: joins the focused tab in studio
    /// mode, the opening page without one.
    pub(super) fn split_pane_studio(&mut self, vertical: bool) {
        if self.studioless() {
            self.opening_show();
            return;
        }
        match self.studio_shell(Some(vertical)) {
            Ok(creation) => self.submit_action(Action::Create(Box::new(creation))),
            Err(error) => self.show_notice("Split unavailable", &error),
        }
    }

    /// `key`'s live shell terminals in shell-number order, on the
    /// studio's own machine — the role keys' tail and the shell
    /// fallback `focus_studio` lands on.
    pub(crate) fn studio_shells_live(&self, key: &StudioKey) -> Vec<String> {
        studio_shells(&self.view, key.machine.as_deref(), &key.id)
            .into_iter()
            .map(|(_, address)| address)
            .filter(|address| {
                self.view
                    .inventory
                    .get(address)
                    .is_some_and(|t| t.state == "live")
            })
            .collect()
    }

    /// The current studio's shell terminals for the role keys —
    /// after the people, in shell-number order, on its own machine.
    pub(crate) fn focused_studio_shells(&self) -> Vec<String> {
        let machines = self.panel_summaries();
        let Some(index) = self.studio.current_index(&machines) else {
            return Vec::new();
        };
        let current = crate::panel::data::studios(&machines)[index];
        self.studio_shells_live(&current.key())
    }
}

#[cfg(test)]
#[path = "studio_shell_tests.rs"]
mod tests;
