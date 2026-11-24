//! U6c: each remote machine's backlot version. A machine's `backlot`
//! document answers first — a stage newer than 0.2.3 publishes
//! `backlot_version` itself, so its machine needs no probe at all.
//! Machines without that field (no stage, or an older stage) are
//! probed through `terminal.run on: <machine>` — the RemoteRunner's
//! locate script finds `backlot` off a sparse PATH and `--version`
//! answers. Probes are cached and never more frequent than
//! `PROBE_EVERY` per machine, are never sent to an offline,
//! disabled or auth-required machine, and re-run on an offline →
//! online transition, on connect and after an install.

use std::collections::{BTreeMap, BTreeSet};
use std::time::{Duration, Instant};

/// At most one automatic probe per machine in this window.
pub(crate) const PROBE_EVERY: Duration = Duration::from_secs(6 * 60 * 60);

/// What a machine's `backlot` looks like, as the sidebar row and
/// the machine menu read it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Backlot {
    /// Nothing is known — no probe ran and the document is silent.
    Unknown,
    /// A probe is in flight.
    Probing,
    /// `backlot --version` answered — or the document said its
    /// version. `exe` is the located binary path when a probe or the
    /// runner's locate cache knows it.
    Installed {
        version: String,
        exe: Option<String>,
    },
    /// The locate probe said there is no `backlot` on the machine.
    Missing,
    /// The probe failed — the text is the machine's own words.
    Error(String),
}

/// The probe answers — remote machines only; the local machine
/// derives its row from `env!("CARGO_PKG_VERSION")` and
/// `current_exe` directly (it is the running binary).
#[derive(Debug, Default)]
pub(crate) struct MachineVersions {
    /// The last probe verdict per machine — a document's
    /// `backlot_version` always outranks it.
    probed: BTreeMap<String, Backlot>,
    /// When the last probe for a machine started — `PROBE_EVERY`
    /// throttles the next automatic one.
    last_probe: BTreeMap<String, Instant>,
    /// In-flight probes — a machine never gets two.
    probing: BTreeSet<String>,
    /// Machines seen offline on an earlier sync — an offline →
    /// online transition re-probes regardless of the cache's age.
    offline: BTreeSet<String>,
}

/// What `want_probes` reads for one machine row.
pub(crate) struct Row<'a> {
    pub name: &'a str,
    /// `machine.list` says `connected` — probing an offline,
    /// disabled or auth-required machine is never wanted.
    pub online: bool,
    /// The machine's `backlot` document carried `backlot_version` —
    /// a newer stage answers for itself.
    pub doc_version: Option<&'a str>,
}

impl MachineVersions {
    /// The state a row draws: the document's `backlot_version`
    /// wins, then a probe verdict, then `Probing`/`Unknown`.
    /// `exe` fills from the runner's locate cache when the caller
    /// knows it.
    pub(crate) fn state(&self, machine: &str, doc_version: Option<&str>) -> Backlot {
        if let Some(version) = doc_version {
            return Backlot::Installed {
                version: version.to_owned(),
                exe: None,
            };
        }
        if let Some(state) = self.probed.get(machine) {
            return state.clone();
        }
        if self.probing.contains(machine) {
            return Backlot::Probing;
        }
        Backlot::Unknown
    }

    /// Which machines want a `backlot --version` probe now.
    /// `rows` is every `machine.list` entry; the answer is empty
    /// unless something is new, stale past `PROBE_EVERY`, or a
    /// machine came back online. A document's `backlot_version`
    /// silences the probe unless the machine just returned.
    pub(crate) fn want_probes(&mut self, now: Instant, rows: &[Row<'_>]) -> Vec<String> {
        // Forget machines the list dropped — the map stays bounded.
        let listed: BTreeSet<&str> = rows.iter().map(|row| row.name).collect();
        self.probed.retain(|m, _| listed.contains(m.as_str()));
        self.last_probe.retain(|m, _| listed.contains(m.as_str()));
        self.probing.retain(|m| listed.contains(m.as_str()));
        self.offline.retain(|m| listed.contains(m.as_str()));
        let mut want = Vec::new();
        for row in rows {
            if !row.online {
                self.offline.insert(row.name.to_owned());
                continue;
            }
            let returned = self.offline.remove(row.name);
            if self.probing.contains(row.name) {
                continue;
            }
            if row.doc_version.is_some() && !returned {
                // The document speaks for the machine — only a
                // reconnect is worth a probe (the locate cache
                // refreshes beside it).
                continue;
            }
            let due = self
                .last_probe
                .get(row.name)
                .is_none_or(|last| now.duration_since(*last) >= PROBE_EVERY);
            if returned || due {
                want.push(row.name.to_owned());
            }
        }
        want
    }

    /// A probe just spawned — the in-flight set and the throttle
    /// stamp both land here, so a slow machine cannot double-probe.
    pub(crate) fn note_probe_start(&mut self, machine: &str, now: Instant) {
        self.probing.insert(machine.to_owned());
        self.last_probe.insert(machine.to_owned(), now);
    }

    /// `backlot --version`'s reply lands — the first line parses or
    /// the machine's own error text is kept.
    pub(crate) fn note_reply(&mut self, machine: &str, out: Result<String, String>) {
        self.probing.remove(machine);
        self.probed.insert(machine.to_owned(), parse_reply(out));
    }

    /// Post-install (or menu-forced): drop the verdict so the next
    /// sync probes again — and clears the located exe the runner
    /// cached for the old binary.
    pub(crate) fn invalidate(&mut self, machine: &str) {
        self.probed.remove(machine);
        self.last_probe.remove(machine);
        self.probing.remove(machine);
    }
}

/// One probe reply's verdict: `backlot 0.2.4` → `Installed`; the
/// runner's "not installed" → `Missing`; anything else `Error`.
fn parse_reply(out: Result<String, String>) -> Backlot {
    match out {
        Ok(text) => match parse_version(&text) {
            Some(version) => Backlot::Installed { version, exe: None },
            None => Backlot::Error(format!("unparseable --version: {}", first_line(&text))),
        },
        Err(e) if e.contains("not installed") => Backlot::Missing,
        Err(e) => Backlot::Error(first_line(&e)),
    }
}

/// `backlot --version`'s first line → the bare version, validated
/// against the same strict semver the installer env gets — a hostile or
/// garbled line never reaches a comparison. A build id after the
/// version — `backlot 0.3.0 (abc123def456)` — is dropped: the probe
/// caches versions, and the update hint compares builds separately.
fn parse_version(text: &str) -> Option<String> {
    let line = text.lines().map(str::trim).find(|l| !l.is_empty())?;
    let line = line.strip_prefix("backlot ").unwrap_or(line).trim();
    let version = line.split_whitespace().next()?;
    crate::update_check::semver(version)
        .is_some()
        .then(|| version.to_owned())
}

fn first_line(text: &str) -> String {
    text.lines()
        .map(str::trim)
        .find(|l| !l.is_empty())
        .unwrap_or("command failed")
        .chars()
        .take(64)
        .collect()
}

/// Whether `installed` is behind `latest` — strict semver on both;
/// an unparseable side answers `false` rather than lying.
pub(crate) fn behind(installed: &str, latest: &str) -> bool {
    match (
        crate::update_check::semver(installed),
        crate::update_check::semver(latest),
    ) {
        (Some(i), Some(l)) => l > i,
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn now() -> Instant {
        Instant::now()
    }

    fn row(name: &str, online: bool) -> Row<'_> {
        Row {
            name,
            online,
            doc_version: None,
        }
    }

    #[test]
    fn a_connected_machine_is_probed_once_per_six_hours() {
        let mut mv = MachineVersions::default();
        let t0 = now();
        let rows = [row("host-b", true)];
        assert_eq!(mv.want_probes(t0, &rows), vec!["host-b".to_owned()]);
        mv.note_probe_start("host-b", t0);
        assert!(
            mv.want_probes(t0, &rows).is_empty(),
            "an in-flight probe never doubles"
        );
        mv.note_reply("host-b", Ok("backlot 0.2.4\n".into()));
        assert!(mv.want_probes(t0, &rows).is_empty(), "fresh cache");
        let t1 = t0 + PROBE_EVERY + Duration::from_secs(1);
        assert_eq!(
            mv.want_probes(t1, &rows),
            vec!["host-b".to_owned()],
            "stale past the window re-probes"
        );
    }

    #[test]
    fn offline_machines_are_never_probed() {
        let mut mv = MachineVersions::default();
        let t0 = now();
        for state in [false, false] {
            assert!(
                mv.want_probes(t0, &[row("host-b", state)]).is_empty(),
                "a non-connected machine is skipped"
            );
        }
        // …but an offline → online transition fires immediately.
        mv.note_reply("host-b", Err("timed out".into()));
        assert_eq!(
            mv.want_probes(t0, &[row("host-b", true)]),
            vec!["host-b".to_owned()]
        );
    }

    #[test]
    fn the_document_version_beats_the_probe() {
        let mut mv = MachineVersions::default();
        let t0 = now();
        let rows = [Row {
            name: "host-b",
            online: true,
            doc_version: Some("0.2.4"),
        }];
        assert!(
            mv.want_probes(t0, &rows).is_empty(),
            "a speaking document wants no probe"
        );
        assert_eq!(
            mv.state("host-b", Some("0.2.4")),
            Backlot::Installed {
                version: "0.2.4".into(),
                exe: None
            }
        );
    }

    #[test]
    fn replies_parse_install_missing_and_error() {
        let mut mv = MachineVersions::default();
        mv.note_reply("a", Ok("backlot 0.2.4".into()));
        mv.note_reply("b", Err("backlot is not installed on b".into()));
        mv.note_reply("c", Err("ssh: connect failed".into()));
        mv.note_reply("d", Ok("<html>".into()));
        assert_eq!(
            mv.state("a", None),
            Backlot::Installed {
                version: "0.2.4".into(),
                exe: None
            }
        );
        assert_eq!(mv.state("b", None), Backlot::Missing);
        assert_eq!(
            mv.state("c", None),
            Backlot::Error("ssh: connect failed".into())
        );
        assert!(matches!(mv.state("d", None), Backlot::Error(_)));
    }

    /// S2 fix: `--version` now prints the build in parentheses; the probe
    /// still reads the version before it.
    #[test]
    fn a_version_line_with_a_build_still_parses() {
        let mut mv = MachineVersions::default();
        mv.note_reply("a", Ok("backlot 0.3.0 (abc123def456)\n".into()));
        mv.note_reply("b", Ok("backlot 0.3.0 (abc123def456-dirty)\n".into()));
        for machine in ["a", "b"] {
            assert_eq!(
                mv.state(machine, None),
                Backlot::Installed {
                    version: "0.3.0".into(),
                    exe: None
                },
                "{machine}"
            );
        }
    }

    #[test]
    fn invalidate_forces_a_reprobe() {
        let mut mv = MachineVersions::default();
        let t0 = now();
        mv.note_probe_start("host-b", t0);
        mv.note_reply("host-b", Ok("backlot 0.2.4".into()));
        mv.invalidate("host-b");
        let rows = [row("host-b", true)];
        assert_eq!(mv.want_probes(t0, &rows), vec!["host-b".to_owned()]);
    }

    #[test]
    fn behind_compares_semver() {
        assert!(behind("0.2.3", "0.2.4"));
        assert!(!behind("0.2.4", "0.2.4"));
        assert!(!behind("0.2.5", "0.2.4"));
        assert!(!behind("v9.9.9.;id;", "0.2.4"), "a hostile side never wins");
        assert!(!behind("0.2.3", "garbage"));
    }
}
