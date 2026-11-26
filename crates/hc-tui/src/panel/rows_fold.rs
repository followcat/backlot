//! Folded-block planning, selected-studio resolution, and fold-summary
//! formatting (`✓ <n> done · <ids>` and `□ <n> closed · <names>`).

use backlot_model::state::TaskStatus;
use backlot_model::summary::StudioSummary;

use super::Panel;
use super::data::StudioRef;
use super::draw_compact::is_active_or_alert_role;
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

/// Live active/alert roles shown inside a non-current folded card.
pub(super) fn folded_roles(studio: &StudioSummary) -> Vec<&backlot_model::summary::RoleSummary> {
    live_roles(studio)
        .into_iter()
        .filter(|r| is_active_or_alert_role(r))
        .collect()
}

/// A folded block's height: `BlockTop`, optional `Owed::Upgrade`, one row per
/// active/alert live role, group dividers, and `BlockBottom`.
pub(super) fn folded_block_height(studio: &StudioSummary) -> usize {
    let roles = folded_roles(studio);
    2 + usize::from(studio.needs_upgrade)
        + super::draw_design::folded_line_count(studio)
        + roles.len()
        + super::rows_fit::divider_count_for(&roles)
}

/// 1-line compact studio categories in `Expanded` mode when a non-current
/// studio has no active/alert roles.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum CompactKind {
    Ready,
    Calm,
    Finished,
    StageDown,
    Offline,
}

/// The rendered shape of a studio block in `Expanded` mode.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum BlockState {
    Full,
    Folded,
    Compact(CompactKind),
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

/// Resolve each studio's shape (`Full`, `Folded`, `Compact`, or `Stacked`):
/// manual `expanded_blocks`/`collapsed_blocks` win; non-current ready/calm/
/// finished/down/offline studios without active roles compress to 1 borderless
/// `Compact` row; active non-current studios draw bordered `Folded` cards;
/// when cards overflow `body_height`, non-focused active cards stack to 1 line.
pub(super) fn block_states(
    studios: &[StudioRef<'_>],
    panel: &Panel,
    current: Option<&StudioKey>,
    body_height: u16,
    compact_of: &dyn Fn(StudioRef<'_>) -> Option<CompactKind>,
    ctx: &dyn Fn(StudioRef<'_>) -> bool,
) -> std::collections::BTreeMap<StudioKey, BlockState> {
    let cursor = cursor_studio(studios, panel, current);
    let selected = selected_studio(studios, panel, current);
    let single = studios.len() <= 1;
    let cursor_inside = |key: &StudioKey| {
        panel
            .selected
            .as_ref()
            .is_some_and(|t| t.studio() == Some(key) && !matches!(t, super::rows::Target::Block(_)))
    };
    let compact_height =
        |key: &StudioKey| 1 + usize::from(panel.finish_confirm.as_ref() == Some(key));

    let initial: Vec<(StudioKey, BlockState)> = studios
        .iter()
        .map(|studio| {
            let key = studio.key();
            let state = if panel.expanded_blocks.contains(&key) {
                BlockState::Full
            } else if let Some(kind) = compact_of(*studio) {
                if panel.collapsed_blocks.contains(&key) {
                    BlockState::Compact(kind)
                } else if single || current == Some(&key) || cursor_inside(&key) {
                    BlockState::Full
                } else {
                    BlockState::Compact(kind)
                }
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
                current,
                ctx(*studio),
                panel.accepted_open(key),
                panel.closed_open(key),
            ),
            BlockState::Folded => folded_block_height(studio.studio),
            BlockState::Compact(_) => compact_height(key),
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
            } else if let Some(kind) = compact_of(*studio) {
                if panel.collapsed_blocks.contains(&key) {
                    BlockState::Compact(kind)
                } else if (!single && (current == Some(&key) || cursor_inside(&key)))
                    || (single && (cursor.as_ref() == Some(&key) || current == Some(&key)))
                {
                    BlockState::Full
                } else {
                    BlockState::Compact(kind)
                }
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
