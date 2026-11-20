// Derived from herdr src/client/shell/config.rs (Apache-2.0).
//! Rewriting the endpoint's chrome-preferences file from live state.
//! Every persisted field is collected — the file always holds the
//! complete current choices, not just the one that changed — and
//! `Option` fields serialize only the choices the user made by hand.
//! A failed write is a notice, never a panic.
use super::*;
use crate::chrome::snapshot::ClientEndpointId;
use crate::preferences::{ClientChromePreferences, ClientRemoteCollapsedGroups};

impl App {
    pub(super) fn persist_chrome_preferences(&mut self) {
        let Some(path) = self.shell.preferences_path.clone() else {
            return;
        };
        let mut collapsed_groups = self
            .shell_state
            .collapsed_groups
            .iter()
            .cloned()
            .collect::<Vec<_>>();
        collapsed_groups.sort();
        let mut remote_collapsed_groups = self
            .shell_state
            .remote_collapsed_groups
            .iter()
            .filter_map(|(endpoint_id, groups)| {
                let ClientEndpointId::Remote(profile_id) = endpoint_id else {
                    return None;
                };
                let mut collapsed_groups = groups.iter().cloned().collect::<Vec<_>>();
                collapsed_groups.sort();
                (!collapsed_groups.is_empty()).then(|| ClientRemoteCollapsedGroups {
                    profile_id: profile_id.to_string(),
                    collapsed_groups,
                })
            })
            .collect::<Vec<_>>();
        remote_collapsed_groups.sort_by(|left, right| left.profile_id.cmp(&right.profile_id));
        let mut new_studio_casts = self.ns_saved_casts.clone();
        new_studio_casts.sort_by(|left, right| left.machine.cmp(&right.machine));
        let preferences = ClientChromePreferences {
            sidebar_width: self.sidebar_width_manual.then_some(self.sidebar_width),
            sidebar_section_split: self
                .sidebar_section_split_manual
                .then_some(self.shell_state.sidebar_section_split),
            sidebar_collapsed: self
                .sidebar_collapsed_manual
                .then_some(self.shell_state.sidebar_collapsed),
            agent_panel_sort: self
                .agent_panel_sort_manual
                .then_some(self.shell.agent_panel_sort),
            collapsed_groups,
            remote_collapsed_groups,
            new_studio_casts,
        };
        if let Err(error) = crate::preferences::store(&path, &preferences) {
            self.show_notice("Client shell state", &error);
        }
    }
}
