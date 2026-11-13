// Fresh code for hc-tui's per-tab floating terminal (the reference's
// one-shot popup is kill-on-switch; this deliberately diverges — see
// docs/parity.md).
//! Float bookkeeping against the published view: sightings, `rm`
//! reaping, owner-gone kills, and label-based adoption of records a
//! restart left behind.

use super::float::{FloatEntry, FloatKey};
use super::popup::{FLOAT_COMMAND_LABEL, FLOAT_OWNER_LABEL, POPUP_LABEL};
use super::*;
use crate::chrome::snapshot::{ClientEndpointId, ClientEndpointStatus, ClientShellEndpoint};
use crate::operations::Action;

impl App {
    /// Split `layout_key(machine, id)` into endpoint + local tab id.
    fn owner_parts(owner: &str) -> Option<(Option<&str>, &str)> {
        let (length, tail) = owner.split_once(':')?;
        let length: usize = length.parse().ok()?;
        let machine = tail.get(..length)?;
        let id = tail.get(length..)?.strip_prefix('/')?;
        Some(((!machine.is_empty()).then_some(machine), id))
    }

    /// The owner tab is really gone: its endpoint published an Online
    /// snapshot listing no such tab. A missing endpoint, a missing
    /// snapshot, or a non-Online status all mean *unknown* — never
    /// kill on unknown.
    pub(super) fn float_owner_gone(endpoints: &[ClientShellEndpoint], owner: &str) -> bool {
        let Some((machine, tab)) = Self::owner_parts(owner) else {
            return false;
        };
        let endpoint_id = machine
            .map(|m| ClientEndpointId::Remote(m.to_string()))
            .unwrap_or(ClientEndpointId::Local);
        let Some(endpoint) = endpoints.iter().find(|e| e.endpoint_id == endpoint_id) else {
            return false;
        };
        if endpoint.status != ClientEndpointStatus::Online {
            return false;
        }
        let Some(snapshot) = &endpoint.snapshot else {
            return false;
        };
        !snapshot.tabs.iter().any(|t| t.tab_id == tab)
    }

    /// Hidden floats after every publish: record sightings, drop
    /// `rm`-reaped exits, kill floats whose owner tab is really gone,
    /// and adopt labeled records a restart left behind.
    pub(super) fn reconcile_floats(&mut self) {
        let online = matches!(self.view.link, hc_proto::watch::Link::Online);
        self.floats.retain(|_key, entry| {
            let term = self.view.inventory.get(&entry.terminal);
            if term.is_some() {
                entry.seen = true;
            }
            // `rm` reaped the finished terminal — the entry follows.
            // An owner-gone kill keeps its entry until the record is
            // actually gone, so rediscovery cannot resurrect it while
            // the kill is in flight (repeats are `allow_gone`-safe).
            if entry.seen && online && term.is_none() {
                self.size_leases.remove(&entry.terminal);
                return false;
            }
            true
        });
        let kill = self
            .floats
            .iter()
            .filter(|(key, entry)| {
                !entry.kill_sent && Self::float_owner_gone(&self.projection.endpoints, &key.owner)
            })
            .map(|(key, entry)| (key.clone(), entry.terminal.clone(), entry.created_ms))
            .collect::<Vec<_>>();
        for (key, terminal, created_ms) in kill {
            self.submit_action(Action::Kill {
                terminal,
                created_ms,
                allow_gone: true,
            });
            // One RPC per record — the entry still outlives the kill
            // until inventory drops the terminal.
            if let Some(entry) = self.floats.get_mut(&key) {
                entry.kill_sent = true;
            }
        }
        // Rediscovery: labeled live records we never tracked. Records
        // without `hc-tui.owner-tab` — popups from before this feature —
        // stay untouched; a label-less orphan is not ours to kill.
        let adoptable = self
            .view
            .inventory
            .terminals()
            .values()
            .filter(|term| {
                term.labels.contains_key(POPUP_LABEL)
                    && term.labels.contains_key(FLOAT_OWNER_LABEL)
                    && term.state == "live"
                    && term.finished_ms.is_none()
            })
            .filter_map(|term| {
                let key = FloatKey {
                    owner: term.labels.get(FLOAT_OWNER_LABEL)?.clone(),
                    command: term
                        .labels
                        .get(FLOAT_COMMAND_LABEL)
                        .cloned()
                        .unwrap_or_default(),
                };
                Some((key, term.address.clone(), term.created_ms))
            })
            .collect::<Vec<_>>();
        for (key, terminal, created_ms) in adoptable {
            let claimed = self
                .floats
                .get(&key)
                .map(|entry| entry.terminal.clone())
                .or_else(|| {
                    self.popup.as_ref().and_then(|popup| {
                        (popup.key.as_ref() == Some(&key)).then(|| popup.terminal.clone())
                    })
                });
            match claimed {
                Some(existing) if existing == terminal => {}
                // A different terminal already claims the key — this
                // record is the duplicate.
                Some(_) => self.submit_action(Action::Kill {
                    terminal,
                    created_ms,
                    allow_gone: true,
                }),
                None => self.hide_float(
                    key,
                    FloatEntry {
                        terminal,
                        width: None,
                        height: None,
                        restore: false,
                        created_ms,
                        seen: true,
                        kill_sent: false,
                    },
                ),
            }
        }
    }
}
