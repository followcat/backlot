//! A `cast` card's execution: the settings change from `studio cast
//! set`, carried out `by: user` with the card's base re-checked at that
//! moment. The card's cast file is read with `studio new`'s parser, the
//! runnable rules re-run, and the recasts and `studio.cast` land in one
//! go — the same path the CLI takes, so a refusal reads the same.
//!
//! Idempotent: the newest `studio.cast` carries the card's `c<n>`
//! marker, so a restart that finds it records `card.done` without
//! re-appending.

use std::path::Path;

use backlot_model::hollywood::{self, Card, CardAction};
use backlot_model::state::StudioState;

use crate::daemon::Stage;
use crate::hcard::Step;

impl Stage {
    pub(crate) fn cast_step(
        &self,
        hdir: &Path,
        sdir: &Path,
        card: &Card,
        sstate: Option<StudioState>,
    ) -> Step {
        let CardAction::Cast { studio, cast, .. } = &card.action else {
            return Step::Fail("not a cast card".into());
        };
        let Some(s) = sstate else {
            return Step::Fail(format!("no studio '{studio}'"));
        };
        // Already done: a crash between the append and `card.done`.
        if s.cast_card.as_deref() == Some(format!("c{}", card.seq).as_str()) {
            return Step::Done;
        }
        // The base: the settings event the card was proposed against.
        if let Some(reason) = hollywood::stale(&card.action, &card.base, Some(&s)) {
            return Step::Stale(reason);
        }
        // The copy the card carries, not the proposer's file it came
        // from: an edit since makes the card stale.
        if let Some(reason) = crate::hcard_snapshot::stale(&card.action) {
            return Step::Stale(reason);
        }
        let path = crate::hcard_new::resolve_draft(hdir, cast);
        let next = match crate::studio_cast::read(&path) {
            Ok(next) => next,
            Err(e) => return Step::Fail(e.message().to_string()),
        };
        let env = crate::cli::Env {
            vars: self.env_vars.clone(),
        };
        match crate::studio_cast::record(
            sdir,
            &self.base,
            &env,
            &crate::cli::Caller::doc_user(studio.clone()),
            &next,
            Some(format!("c{}", card.seq)),
        ) {
            Ok(_) => Step::Done,
            Err(e) => Step::Fail(e.message().to_string()),
        }
    }
}
