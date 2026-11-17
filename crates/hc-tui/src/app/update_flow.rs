//! The update flow's async events — release-notes fetches and
//! (later) installer lifecycle — land on `update_rx` inside the
//! select loop, keeping every network/process wait off the event
//! loop and silent on failure.

use super::*;
use crate::update_state::CachedRelease;

/// Fetches the release list for the running version. Injected so
/// tests can answer canned JSON; the test build's default never
/// reaches the network (`default_notes_fetch`).
pub(crate) type NotesFetch = Arc<dyn Fn() -> BoxFut<Option<Vec<CachedRelease>>> + Send + Sync>;

/// What a background update worker reports back.
#[derive(Debug)]
pub(crate) enum UpdateEvent {
    /// A GitHub releases fetch resolved. `releases` is `None` on any
    /// failure — the modal falls back to its offline body and the
    /// cache keeps its previous entries.
    Notes {
        releases: Option<Vec<CachedRelease>>,
    },
    /// One installer output line from the run `generation`.
    Output { generation: u64, line: String },
    /// The installer finished; `code` is `None` when a signal stopped it.
    Exited { generation: u64, code: Option<i32> },
    /// The `--version` check finished: `hc-tui <v>` or nothing.
    Verified {
        generation: u64,
        version: Option<String>,
    },
}

/// The client's real fetch; the test build answers offline so no unit
/// test can reach GitHub, injected or not.
#[cfg(not(test))]
pub(crate) fn default_notes_fetch() -> NotesFetch {
    Arc::new(|| {
        Box::pin(crate::update_notes::fetch_releases(env!(
            "CARGO_PKG_VERSION"
        )))
    })
}

#[cfg(test)]
pub(crate) fn default_notes_fetch() -> NotesFetch {
    Arc::new(|| Box::pin(async { None }))
}

impl App {
    /// Installs the release-list fetch — tests inject a canned answer.
    pub fn set_notes_fetch(&mut self, fetch: NotesFetch) {
        self.notes_fetch = fetch;
    }

    /// Release notes are cached under the latest tag they were fetched
    /// for. A cache hit is free; `force` (the modal opening) always
    /// refetches so the latest tag is refreshed at once.
    pub(super) fn kick_notes_fetch(&mut self, force: bool) {
        let Some(latest) = self.update_latest.clone() else {
            return;
        };
        let tag = format!("v{latest}");
        if self.update_notes_fetching
            || (!force && self.update_state.releases_for_tag.as_deref() == Some(tag.as_str()))
        {
            return;
        }
        self.update_notes_fetching = true;
        let tx = self.update_tx.clone();
        let fetch = Arc::clone(&self.notes_fetch);
        tokio::spawn(async move {
            let releases = fetch().await;
            let _ = tx.send(UpdateEvent::Notes { releases }).await;
        });
    }

    /// A notes fetch landed: fresh releases replace the cache under
    /// the tag of their newest entry, and a newer newest entry moves
    /// the latest known version. A failure keeps the previous cache.
    pub(super) fn on_update_event(&mut self, event: UpdateEvent) {
        match event {
            UpdateEvent::Notes { releases } => self.on_notes(releases),
            other => self.on_install_event(other),
        }
    }

    fn on_notes(&mut self, releases: Option<Vec<CachedRelease>>) {
        self.update_notes_fetching = false;
        let Some(releases) = releases else {
            self.sched.mark_dirty(Instant::now());
            return;
        };
        if let Some(newest) = releases.first() {
            self.update_state.releases_for_tag = Some(format!("v{}", newest.version));
            let outranks = self.update_latest.as_deref().is_none_or(|latest| {
                crate::update_check::semver(&newest.version) > crate::update_check::semver(latest)
            });
            if outranks {
                self.update_latest = Some(newest.version.clone());
            }
        }
        self.update_state.releases = releases;
        self.persist_update_state();
        self.sched.mark_dirty(Instant::now());
    }
}
