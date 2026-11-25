//! Folded-block planning, selected-studio resolution, and fold-summary
//! formatting (`✓ <n> done · <ids>` and `□ <n> closed · <names>`).

use backlot_model::state::TaskStatus;
use backlot_model::summary::StudioSummary;

use super::Panel;
use super::data::StudioRef;
use super::rows_fit::{block_height, closed_roles, live_roles, represented};
use crate::studio::StudioKey;

/// Closed role name tokens in first-seen order (`actor-a×3` when
/// repeated), plus total closed role count.
pub(super) fn closed_fold_summary(studio: &StudioSummary) -> (usize, Vec<String>) {
    let mut total = 0usize;
    let mut counts: Vec<(&str, usize)> = Vec::new();
    for role in closed_roles(studio) {
        total += 1;
        if let Some((_, n)) = counts.iter_mut().find(|(name, _)| *name == role.name) {
            *n += 1;
        } else {
            counts.push((role.name.as_str(), 1));
        }
    }
    let tokens = counts
        .into_iter()
        .map(|(name, n)| {
            if n > 1 {
                format!("{name}×{n}")
            } else {
                name.to_string()
            }
        })
        .collect();
    (total, tokens)
}

/// Ids of accepted tasks behind the done fold in summary order.
pub(super) fn accepted_fold_ids(studio: &StudioSummary) -> Vec<&str> {
    studio
        .tasks
        .iter()
        .filter(|t| {
            matches!(
                t.status,
                TaskStatus::Accepted | TaskStatus::PartiallyAccepted
            )
        })
        .filter(|t| !represented(studio, t))
        .map(|t| t.id.as_str())
        .collect()
}

/// A folded block's height: `BlockTop`, one row per live role, and
/// `BlockBottom`.
pub(super) fn folded_block_height(studio: &StudioSummary) -> usize {
    2 + live_roles(studio).len()
}

/// The rendered shape of a studio block in `Expanded` mode.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum BlockState {
    Full,
    Folded,
    Stacked,
}

/// Studio holding the dock cursor (or owning a `Target::Stage` machine).
fn cursor_studio(
    studios: &[StudioRef<'_>],
    panel: &Panel,
    current: Option<&StudioKey>,
) -> Option<StudioKey> {
    match panel.selected.as_ref() {
        Some(super::rows::Target::Stage(machine)) => current
            .filter(|key| {
                studios
                    .iter()
                    .any(|s| &s.key() == *key && s.machine == machine.as_deref())
            })
            .cloned()
            .or_else(|| {
                studios
                    .iter()
                    .find(|s| s.machine == machine.as_deref())
                    .map(|s| s.key())
            }),
        Some(target) => target
            .studio()
            .filter(|key| studios.iter().any(|s| &s.key() == *key))
            .cloned(),
        None => None,
    }
}

/// Which studio is selected: the cursor's studio (or a `Target::Stage`
/// machine's studio), else the current (`▸`) studio, else the first
/// studio in document order.
pub(super) fn selected_studio(
    studios: &[StudioRef<'_>],
    panel: &Panel,
    current: Option<&StudioKey>,
) -> Option<StudioKey> {
    cursor_studio(studios, panel, current)
        .or_else(|| {
            current
                .filter(|key| studios.iter().any(|s| &s.key() == *key))
                .cloned()
        })
        .or_else(|| studios.first().map(|s| s.key()))
}

/// Resolve each studio's shape (`Full`, `Folded`, or `Stacked`):
/// manual `expanded_blocks`/`collapsed_blocks` win; a solo studio
/// never folds; unselected studios fold to header + live roles when
/// they fit `body_height`, and fall back to `Stacked` when not.
pub(super) fn block_states(
    studios: &[StudioRef<'_>],
    panel: &Panel,
    current: Option<&StudioKey>,
    body_height: u16,
    ctx: &dyn Fn(StudioRef<'_>) -> bool,
) -> std::collections::BTreeMap<StudioKey, BlockState> {
    let cursor = cursor_studio(studios, panel, current);
    let selected = selected_studio(studios, panel, current);
    let single = studios.len() <= 1;
    let initial: Vec<(StudioKey, BlockState)> = studios
        .iter()
        .map(|studio| {
            let key = studio.key();
            let state = if panel.expanded_blocks.contains(&key) {
                BlockState::Full
            } else if panel.collapsed_blocks.contains(&key) {
                BlockState::Stacked
            } else if single || selected.as_ref() == Some(&key) {
                BlockState::Full
            } else {
                BlockState::Folded
            };
            (key, state)
        })
        .collect();
    let needed: usize = studios
        .iter()
        .zip(initial.iter())
        .map(|(studio, (key, state))| match state {
            BlockState::Full => block_height(
                *studio,
                panel,
                ctx(*studio),
                panel.accepted_open(key),
                panel.closed_open(key),
            ),
            BlockState::Folded => folded_block_height(studio.studio),
            BlockState::Stacked => 1,
        })
        .sum();
    if body_height == 0 || needed <= body_height as usize {
        return initial.into_iter().collect();
    }
    studios
        .iter()
        .map(|studio| {
            let key = studio.key();
            let state = if panel.expanded_blocks.contains(&key) {
                BlockState::Full
            } else if panel.collapsed_blocks.contains(&key) {
                BlockState::Stacked
            } else if (!single && selected.as_ref() == Some(&key))
                || (single && (cursor.as_ref() == Some(&key) || current == Some(&key)))
            {
                BlockState::Full
            } else {
                BlockState::Stacked
            };
            (key, state)
        })
        .collect()
}
