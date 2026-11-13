// Fresh code for hc-tui's per-tab floating terminal (the reference's
// one-shot popup is kill-on-switch; this deliberately diverges — see
// docs/parity.md).
//! Per-tab persistent floating terminals. A `persistent` popup binding
//! toggles that tab's float instead of running a one-shot: hidden, the
//! program keeps running behind a `Role::SizeLease` attach in
//! `self.size_leases`, so its dimensions never collapse to the core's
//! headless default while parked. `hc-tui.owner-tab` /
//! `hc-tui.command` labels let a restarted client adopt the record
//! without a resize.

use super::pane::{Pane, Role, Spec};
use super::popup::{FLOAT_COMMAND_LABEL, FLOAT_OWNER_LABEL, POPUP_LABEL, PopupState, popup_fit};
use super::*;
use crate::config::CustomCommandKeybind;
use crate::operations::Action;
use crate::popup_size::{PopupSize, resolve_popup_geometry};
use std::collections::BTreeMap;

/// One persistent float: the owning tab plus the binding label that
/// started it, so two popup bindings on one tab stay distinct.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub(super) struct FloatKey {
    /// `layout_key(machine, tab_id)` — machine-qualified, so a remote
    /// tab never collides with a local one.
    pub owner: String,
    /// The `[[keys.command]]` label, mirrored as `hc-tui.command` for
    /// rediscovery after a restart.
    pub command: String,
}

/// A persistent float currently hidden: the program runs on behind a
/// `SizeLease` until the binding shows it again or the owning tab
/// disappears for good.
pub(super) struct FloatEntry {
    pub terminal: String,
    /// Requested popup geometry, reapplied on the next show.
    pub width: Option<PopupSize>,
    pub height: Option<PopupSize>,
    /// Was visible when focus left — a tab round-trip re-shows it; an
    /// explicit Alt+F hide stays hidden.
    pub restore: bool,
    /// Core `created_ms` — incarnation match for the owner-gone kill.
    pub created_ms: u64,
    /// Inventory-lag guard, same as `PopupState.seen`.
    pub seen: bool,
    /// The owner-gone kill already left — send the RPC once; the entry
    /// stays until the record actually leaves inventory.
    pub kill_sent: bool,
}

impl App {
    /// `layout_key` of the tab a popup opened right now belongs to: the
    /// focused terminal's machine plus the projection's focused tab.
    pub(super) fn focused_owner_key(&self) -> Option<String> {
        let tab = self.projection.snapshot.focused_tab_id.as_deref()?;
        let machine = self
            .focused
            .as_deref()
            .and_then(|f| f.split_once('/').map(|(m, _)| m));
        Some(crate::project::layout_key(machine, tab))
    }

    /// Where a fresh float starts: the repository root of the focused
    /// terminal's effective cwd, else the workspace folder, else the
    /// effective cwd — every lookup resolved on the tab's machine.
    pub(super) fn float_cwd(&self) -> Option<String> {
        let terminal = self
            .focused
            .as_deref()
            .and_then(|id| self.view.inventory.get(id))?;
        let effective = terminal
            .foreground
            .as_ref()
            .map(|f| f.cwd.as_str())
            .filter(|cwd| !cwd.is_empty())
            .unwrap_or(terminal.cwd.as_str());
        // Facts describe `facts.cwd`; only a matching record may claim
        // the effective cwd's checkout — right after a `cd` the stale
        // record still names the old directory.
        if let Some(facts) = terminal
            .repository
            .as_ref()
            .filter(|facts| facts.cwd == effective)
            && let Some(checkout) = &facts.checkout
        {
            return Some(checkout.repo_root.clone());
        }
        let workspace = self
            .projection
            .snapshot
            .focused_workspace_id
            .as_deref()
            .and_then(|id| {
                self.projection
                    .snapshot
                    .workspaces
                    .iter()
                    .find(|w| w.workspace_id == id)
            });
        workspace
            .map(|w| w.new_workspace_cwd.clone())
            .filter(|cwd| !cwd.is_empty())
            .or_else(|| Some(effective.to_string()))
    }

    /// A `persistent` popup binding toggles that tab's float: hide the
    /// shown one, show the hidden one, or run a fresh terminal.
    pub(super) fn spawn_persistent_float(&mut self, command: &CustomCommandKeybind) {
        // The key arm swallows presses while a run flies; menus can
        // still reach here — never double-spawn.
        if self.popup_pending.is_some() {
            return;
        }
        let Some(owner) = self.focused_owner_key() else {
            return;
        };
        let key = FloatKey {
            owner,
            command: command.label.clone(),
        };
        if self
            .popup
            .as_ref()
            .is_some_and(|p| p.key.as_ref() == Some(&key))
        {
            self.demote_float(false);
            return;
        }
        if self.floats.contains_key(&key) {
            self.promote_float(&key);
            return;
        }
        self.dismiss_popup();
        self.spawn_float_run(command, key);
    }

    /// `terminal.run` for a fresh float — `on_popup_spawned` attaches
    /// it Active when the owner is still focused, parked hidden when a
    /// switch raced the run.
    fn spawn_float_run(&mut self, command: &CustomCommandKeybind, key: FloatKey) {
        let geometry = resolve_popup_geometry(command.width, command.height, self.frame_area());
        let machine = self
            .focused
            .as_deref()
            .and_then(|f| f.split_once('/').map(|(m, _)| m.to_owned()));
        let mut params = hc_proto::terminal::RunParams {
            line: Some(command.command.clone()),
            cwd: self.float_cwd(),
            rm: true,
            keep: true,
            no_notify: true,
            no_parent: true,
            env: crate::app::creation::program_env(),
            labels: BTreeMap::from([
                (POPUP_LABEL.into(), "1".into()),
                (FLOAT_OWNER_LABEL.into(), key.owner.clone()),
                (FLOAT_COMMAND_LABEL.into(), key.command.clone()),
            ]),
            display: self.host_facts.display(),
            ..Default::default()
        };
        params.on = machine;
        if let Some(geometry) = geometry {
            params.size = Some(hc_proto::terminal::InitialSize {
                cols: geometry.terminal_cols,
                rows: geometry.terminal_rows,
            });
        }
        self.popup_pending = Some(super::popup::PopupPending {
            width: command.width,
            height: command.height,
            persistent: true,
            owner: key.owner,
            command: key.command,
        });
        self.submit_action(Action::RunPopup { params });
    }

    /// Replace whatever is shown: a persistent float demotes (it was
    /// visible, so a tab round-trip restores it), anything else is
    /// killed like today's `close_popup`.
    pub(super) fn dismiss_popup(&mut self) {
        match self.popup.as_ref() {
            Some(popup) if popup.persistent => self.demote_float(true),
            Some(_) => self.close_popup(),
            None => {}
        }
    }

    /// Hide the shown float: the pane stays alive, parked on a
    /// `SizeLease` attach at the popup's resolved size — the program
    /// never sees the core's headless default while hidden. `restore`
    /// remembers "was visible": a tab round-trip re-shows it, an
    /// explicit Alt+F hide does not.
    pub(super) fn demote_float(&mut self, restore: bool) {
        let Some(popup) = self.popup.take() else {
            return;
        };
        if !popup.persistent {
            self.popup = Some(popup);
            self.close_popup();
            return;
        }
        self.popup_selection = None;
        self.popup_gesture = None;
        let mut pane = popup.pane;
        if let Some(geometry) = resolve_popup_geometry(popup.width, popup.height, self.frame_area())
        {
            popup_fit(&mut pane, geometry);
        }
        pane.want(Some(Spec {
            terminal: popup.terminal.clone(),
            role: Role::SizeLease,
        }));
        pane.on_view(&self.view);
        self.size_leases.insert(popup.terminal.clone(), pane);
        self.floats.insert(
            popup.key.expect("persistent popups carry a key"),
            FloatEntry {
                terminal: popup.terminal,
                width: popup.width,
                height: popup.height,
                restore,
                created_ms: popup.created_ms,
                seen: popup.seen,
                kill_sent: false,
            },
        );
        self.sched.mark_dirty(Instant::now());
    }

    /// Show a hidden float: the lease leaves `size_leases`, the pane
    /// reattaches `Role::Active` at the requested geometry.
    fn promote_float(&mut self, key: &FloatKey) {
        let Some(entry) = self.floats.remove(key) else {
            return;
        };
        self.dismiss_popup();
        // Only the last-shown float of an owner may come back.
        for (other_key, other) in self.floats.iter_mut() {
            if other_key.owner == key.owner {
                other.restore = false;
            }
        }
        let mut pane = self.size_leases.remove(&entry.terminal).unwrap_or_else(|| {
            let mut pane = Pane::new(self.pane_tx.clone(), self.open_attach.clone());
            pane.set_host_facts(self.host_facts.clone());
            pane
        });
        pane.want(Some(Spec {
            terminal: entry.terminal.clone(),
            role: Role::Active,
        }));
        pane.on_view(&self.view);
        if let Some(geometry) = resolve_popup_geometry(entry.width, entry.height, self.frame_area())
        {
            popup_fit(&mut pane, geometry);
        }
        self.popup = Some(PopupState {
            pane,
            terminal: entry.terminal,
            width: entry.width,
            height: entry.height,
            owner_tab: None,
            persistent: true,
            key: Some(key.clone()),
            created_ms: entry.created_ms,
            seen: entry.seen,
        });
        self.sched.mark_dirty(Instant::now());
    }

    /// Attach a just-run float Active — the visible arm of
    /// `on_popup_spawned` for a persistent pending.
    pub(super) fn show_float(
        &mut self,
        key: FloatKey,
        terminal: String,
        width: Option<PopupSize>,
        height: Option<PopupSize>,
        created_ms: u64,
    ) {
        let mut pane = Pane::new(self.pane_tx.clone(), self.open_attach.clone());
        pane.set_host_facts(self.host_facts.clone());
        pane.want(Some(Spec {
            terminal: terminal.clone(),
            role: Role::Active,
        }));
        pane.on_view(&self.view);
        if let Some(geometry) = resolve_popup_geometry(width, height, self.frame_area()) {
            popup_fit(&mut pane, geometry);
        }
        self.drop_float(&key, Some(terminal.as_str()));
        // Only the last-shown float of an owner may come back.
        for (other_key, other) in self.floats.iter_mut() {
            if other_key.owner == key.owner {
                other.restore = false;
            }
        }
        self.popup = Some(PopupState {
            pane,
            terminal,
            width,
            height,
            owner_tab: None,
            persistent: true,
            key: Some(key),
            created_ms,
            seen: false,
        });
        self.sched.mark_dirty(Instant::now());
    }

    /// Park a terminal on a hidden SizeLease — a late `Ran` or a
    /// rediscovered record. The lease takes the record's own size when
    /// known (adoption must not resize), else the popup geometry.
    pub(super) fn hide_float(&mut self, key: FloatKey, entry: FloatEntry) {
        let mut lease = Pane::new(self.pane_tx.clone(), self.open_attach.clone());
        lease.set_host_facts(self.host_facts.clone());
        let (cols, rows) = self
            .view
            .inventory
            .get(&entry.terminal)
            .map(|term| (term.cols, term.rows))
            .or_else(|| {
                resolve_popup_geometry(entry.width, entry.height, self.frame_area())
                    .map(|geometry| (geometry.terminal_cols, geometry.terminal_rows))
            })
            .unwrap_or((80, 24));
        lease.allocated(cols.max(1), rows.max(1));
        lease.want(Some(Spec {
            terminal: entry.terminal.clone(),
            role: Role::SizeLease,
        }));
        lease.on_view(&self.view);
        let terminal = entry.terminal.clone();
        self.drop_float(&key, Some(terminal.as_str()));
        // A fresh arrival parks as the owner's restore candidate.
        if entry.restore {
            for (other_key, other) in self.floats.iter_mut() {
                if other_key.owner == key.owner {
                    other.restore = false;
                }
            }
        }
        self.floats.insert(key, entry);
        self.size_leases.insert(terminal, lease);
        self.sched.mark_dirty(Instant::now());
    }

    /// A same-key entry claims a different terminal — a stale duplicate
    /// from a lost local map or a racing client. Kill it so one owner
    /// never keeps two terminals.
    fn drop_float(&mut self, key: &FloatKey, keep_terminal: Option<&str>) {
        let Some(old) = self.floats.remove(key) else {
            return;
        };
        if Some(old.terminal.as_str()) == keep_terminal {
            self.floats.insert(key.clone(), old);
            return;
        }
        self.size_leases.remove(&old.terminal);
        self.submit_action(Action::Kill {
            terminal: old.terminal,
            created_ms: old.created_ms,
            allow_gone: true,
        });
    }

    /// Host resize: refit each hidden float's `SizeLease` pane from its
    /// stored popup geometry — the percent/cell request follows the new
    /// frame just like the visible popup's refit.
    pub(super) fn floats_resize(&mut self, area: ratatui::layout::Rect) {
        for entry in self.floats.values() {
            if let Some(geometry) = resolve_popup_geometry(entry.width, entry.height, area)
                && let Some(lease) = self.size_leases.get_mut(&entry.terminal)
            {
                popup_fit(lease, geometry);
            }
        }
    }

    /// Focus moved (tab, workspace, endpoint): the shown float of the
    /// old owner is parked — program alive — and the new owner's float
    /// re-shows if it was visible when left. `refresh_projection` calls
    /// this once the snapshot settles, before `reconcile_popups`.
    pub(super) fn sync_float_focus(&mut self) {
        let focused = self.focused_owner_key();
        if self.popup.as_ref().is_some_and(|popup| {
            popup.persistent
                && popup
                    .key
                    .as_ref()
                    .is_some_and(|key| Some(&key.owner) != focused.as_ref())
        }) {
            self.demote_float(true);
        }
        let Some(owner) = focused else {
            return;
        };
        let restore = if self.popup.is_none() {
            self.floats
                .iter()
                .find(|(key, entry)| key.owner == owner && entry.restore)
                .map(|(key, _)| key.clone())
        } else {
            None
        };
        if let Some(key) = restore {
            self.promote_float(&key);
        }
    }
}
