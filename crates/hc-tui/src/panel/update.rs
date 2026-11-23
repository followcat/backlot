//! The stage's newer-version hint (S2): after an upgrade the stage
//! restarts on the new binary while a running client keeps the old one.
//! When a source says a newer version is installed than the running
//! client's, the lot says so on one dim line.
//!
//! The comparison is strict semver first. Equal versions fall back to
//! the build id the stage publishes, when it publishes one — two
//! builds of the same version still count as different code.

use crate::update_check::semver;

/// One source's claim, and what the lot shows for it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct UpdateHint {
    /// The version to print, with `+<build>` when the build id is what
    /// made it newer.
    pub version: String,
}

impl UpdateHint {
    /// The dim line at the foot of the lot.
    pub(crate) fn text(&self) -> String {
        format!(
            "backlot {} installed — restart the client to update (q, then backlot)",
            self.version
        )
    }
}

/// The newest candidate that outranks the running client, if any.
///
/// `candidates` are `(version, build id)` pairs from the local stage's
/// document and any `backlot --version` probe answer. A candidate wins
/// on a strictly greater semver; on an equal one only when both it and
/// the running client publish a build id and they differ. An older
/// stage never wins, and an unparseable side is ignored rather than
/// guessed at.
pub(crate) fn newest(
    running: &str,
    running_build: Option<&str>,
    candidates: &[(String, Option<String>)],
) -> Option<UpdateHint> {
    let running = semver(running)?;
    let mut best: Option<((u32, u32, u32), UpdateHint)> = None;
    for (version, build) in candidates {
        let Some(candidate) = semver(version) else {
            continue;
        };
        let same_version = candidate == running;
        let build_differs = same_version
            && matches!(
                (running_build, build.as_deref()),
                (Some(running), Some(candidate)) if running != candidate
            );
        if candidate < running || (same_version && !build_differs) {
            continue;
        }
        let shown = if same_version {
            format!("{version}+{}", build.as_deref().unwrap_or_default())
        } else {
            version.clone()
        };
        if best.as_ref().is_none_or(|(seen, _)| candidate > *seen) {
            best = Some((candidate, UpdateHint { version: shown }));
        }
    }
    best.map(|(_, hint)| hint)
}
