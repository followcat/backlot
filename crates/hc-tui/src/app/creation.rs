//! Create neutral labeled terminals without rewriting the foreign document.
use super::*;
use crate::operations::{Action, Change, Creation, Member, Promotion};
use std::collections::BTreeMap;

fn opaque_id() -> Result<String, String> {
    use std::io::Read;
    let mut bytes = [0u8; 16];
    std::fs::File::open("/dev/urandom")
        .and_then(|mut f| f.read_exact(&mut bytes))
        .map_err(|e| e.to_string())?;
    Ok(bytes.iter().map(|b| format!("{b:02x}")).collect())
}
fn raw_id(id: &str, kind: &str) -> Option<String> {
    let tail = id.strip_prefix(kind)?.strip_prefix(':')?;
    let (length, raw) = tail.split_once(':')?;
    (raw.len() == length.parse::<usize>().ok()?).then(|| raw.to_owned())
}

impl App {
    pub(super) fn creation(
        &self,
        vertical: Option<bool>,
        zoomed: bool,
    ) -> Result<Creation, String> {
        let terminal = self
            .focused
            .as_ref()
            .and_then(|id| self.view.inventory.get(id))
            .ok_or("No focused terminal")?;
        let machine = terminal.address.split_once('/').map(|(m, _)| m);
        let terms = self
            .view
            .inventory
            .terminals()
            .values()
            .filter(|t| t.address.split_once('/').map(|(m, _)| m) == machine)
            .collect::<Vec<_>>();
        let app = self
            .view
            .documents
            .get(&hc_proto::watch::DocKey {
                machine: machine.map(str::to_owned),
                name: "herdr-app".into(),
            })
            .map(|d| d.value.clone())
            .unwrap_or_default();
        let spaces = crate::structure::read(&terms, &app, machine);
        let snapshot = &self.projection.snapshot;
        let space = spaces
            .iter()
            .find(|s| Some(&s.id) == snapshot.focused_workspace_id.as_ref())
            .ok_or("Workspace changed")?;
        let current_tab = snapshot.focused_tab_id.as_ref().ok_or("No focused tab")?;
        let raw_space = raw_id(&space.id, "space")
            .map(Ok)
            .unwrap_or_else(opaque_id)?;
        let mut labels_by_tab = BTreeMap::new();
        for tab in &space.tabs {
            let suffix = tab
                .id
                .strip_prefix(&format!("{}/", space.id))
                .unwrap_or_default();
            let raw = raw_id(suffix, "tab").map(Ok).unwrap_or_else(opaque_id)?;
            labels_by_tab.insert(crate::project::layout_key(machine, &tab.id), raw);
        }
        let members = space
            .tabs
            .iter()
            .flat_map(|tab| {
                tab.root
                    .terminals()
                    .into_iter()
                    .map(|id| (id, crate::project::layout_key(machine, &tab.id)))
            })
            .filter_map(|(id, tab)| {
                let term = self.view.inventory.get(&id)?;
                Some(Member {
                    terminal: id,
                    tab,
                    labels: term.labels.clone(),
                })
            })
            .collect::<Vec<_>>();
        let promote = members.iter().any(|m| {
            m.labels.get("herdr.space") != Some(&raw_space)
                || m.labels.get("herdr.tab") != labels_by_tab.get(&m.tab)
        });
        let raw_tab = if vertical.is_none() {
            opaque_id()?
        } else {
            labels_by_tab
                .get(&crate::project::layout_key(machine, current_tab))
                .cloned()
                .ok_or("Tab changed")?
        };
        let own_space = format!("space:{}:{raw_space}", raw_space.len());
        let own_tab = format!("{own_space}/tab:{}:{raw_tab}", raw_tab.len());
        let change = vertical.map_or_else(
            || Change::NewTab {
                terminal: String::new(),
                name: String::new(),
            },
            |vertical| Change::Split {
                target: terminal.address.clone(),
                terminal: String::new(),
                vertical,
                zoomed,
            },
        );
        let mut edit = self.layout_edit(change).ok_or("No active layout")?;
        if !promote || vertical.is_none() {
            edit.space = crate::project::layout_key(machine, &own_space);
            edit.tab = crate::project::layout_key(machine, &own_tab);
        }
        let cwd = if vertical.is_none() {
            space.folder.clone()
        } else {
            terminal
                .foreground
                .as_ref()
                .map(|f| &f.cwd)
                .filter(|s| !s.is_empty())
                .unwrap_or(&terminal.cwd)
                .clone()
        };
        Ok(Creation {
            params: hc_proto::terminal::RunParams {
                on: machine.map(str::to_owned),
                cwd: Some(cwd),
                no_parent: true,
                no_notify: true,
                keep: true,
                labels: BTreeMap::from([
                    ("herdr.space".into(), raw_space.clone()),
                    ("herdr.tab".into(), raw_tab),
                    ("herdr.seen".into(), "0".into()),
                ]),
                display: self.host_facts.display(),
                ..Default::default()
            },
            edit,
            order: space.tabs.iter().map(|t| t.id.clone()).collect(),
            space_adoption: None,
            promotion: promote.then(|| Promotion {
                key: crate::project::layout_key(machine, &space.id),
                proposal: crate::structure::Adoption {
                    machine: machine.map(str::to_owned),
                    space: raw_space,
                    tabs: labels_by_tab,
                    name: space.name.clone(),
                    custom_label: space.custom_name,
                    ..Default::default()
                },
                members,
            }),
        })
    }
    /// A brand-new workspace: one terminal, one tab, fresh stable space and
    /// tab identities, and an adoption entry recording its label — the
    /// reference's `Method::WorkspaceCreate` plus its workspace-label write.
    pub(super) fn workspace_creation(&self) -> Result<(Creation, String), String> {
        let terminal = self
            .focused
            .as_ref()
            .and_then(|id| self.view.inventory.get(id))
            .ok_or("No focused terminal")?;
        let machine = terminal.address.split_once('/').map(|(m, _)| m);
        let snapshot = &self.projection.snapshot;
        let workspace = snapshot
            .focused_workspace_id
            .as_ref()
            .and_then(|id| snapshot.workspaces.iter().find(|w| &w.workspace_id == id))
            .ok_or("No focused workspace")?;
        // The reference creates the new workspace in the focused workspace's
        // `new_workspace_cwd` (checkout root, else its folder).
        let cwd = if workspace.new_workspace_cwd.is_empty() {
            terminal.cwd.clone()
        } else {
            workspace.new_workspace_cwd.clone()
        };
        let suggested = suggested_name(&cwd);
        let raw_space = opaque_id()?;
        let raw_tab = opaque_id()?;
        let own_space = format!("space:{}:{raw_space}", raw_space.len());
        let own_tab = format!("{own_space}/tab:{}:{raw_tab}", raw_tab.len());
        let tab_key = crate::project::layout_key(machine, &own_tab);
        let space_key = crate::project::layout_key(machine, &own_space);
        let creation = Creation {
            params: hc_proto::terminal::RunParams {
                on: machine.map(str::to_owned),
                cwd: Some(cwd),
                no_parent: true,
                no_notify: true,
                keep: true,
                labels: BTreeMap::from([
                    ("herdr.space".into(), raw_space.clone()),
                    ("herdr.tab".into(), raw_tab.clone()),
                    ("herdr.seen".into(), "0".into()),
                ]),
                display: self.host_facts.display(),
                ..Default::default()
            },
            edit: crate::operations::Edit {
                tab: tab_key.clone(),
                space: space_key,
                fallback: crate::structure::Tree::pane("pending-created-terminal"),
                incarnations: BTreeMap::new(),
                change: Change::NewTab {
                    terminal: String::new(),
                    name: String::new(),
                },
            },
            promotion: None,
            space_adoption: Some(crate::structure::Adoption {
                machine: machine.map(str::to_owned),
                space: raw_space,
                tabs: BTreeMap::from([(tab_key, raw_tab)]),
                name: suggested.clone(),
                custom_label: false,
                ..Default::default()
            }),
            order: Vec::new(),
        };
        Ok((creation, suggested))
    }
    pub(super) fn split_pane(&mut self, vertical: bool) {
        match self.creation(Some(vertical), false) {
            Ok(creation) => self.submit_action(Action::Create(Box::new(creation))),
            Err(error) => self.show_notice("Split unavailable", &error),
        }
    }
}

/// The reference's fallback workspace label for a non-checkout cwd: the
/// path's basename, `~` for the home directory, `workspace` otherwise.
fn suggested_name(cwd: &str) -> String {
    let home = std::env::var_os("HOME").map(|h| h.to_string_lossy().into_owned());
    if home.as_deref().is_some_and(|h| !h.is_empty() && h == cwd) {
        return "~".into();
    }
    cwd.trim_end_matches('/')
        .rsplit('/')
        .next()
        .filter(|b| !b.is_empty())
        .unwrap_or("workspace")
        .to_owned()
}
