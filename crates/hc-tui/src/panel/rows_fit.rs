//! The row model's fit half: how tall each block is, which blocks
//! earn an expanded slot inside the body's height, the studio's
//! mark counts and the strip's never-blank role pick. Pure
//! projections over `StudioSummary` + `Panel` — `rows.rs` assembles
//! what these decide.

use backlot_model::state::TaskStatus;
use backlot_model::summary::{RoleSummary, StudioSummary, TaskSummary};

use super::Panel;
use super::data::StudioRef;
use super::draw_compact::is_active_or_alert_role;
use super::rows::*;
use super::rows_fold::{BlockState, accepted_fold_ids, folded_block_height, selected_studio};
use crate::studio::StudioKey;
use crate::studio::glyphs::Person;

/// The order a stacked block's mark counts follow — the urgents
/// first, then the working and quiet states.
const MARK_ORDER: [Person; 13] = [
    Person::UserPrompt,
    Person::DirPrompt,
    Person::NeedsYou,
    Person::Asks,
    Person::Stuck,
    Person::Failed,
    Person::Lost,
    Person::Working,
    Person::Slot,
    Person::Idle,
    Person::Ready,
    Person::Resumable,
    Person::Closed,
];

/// A stacked block's mark counts — `(mark, n)` for each nonzero mark
/// in `MARK_ORDER`.
pub(crate) fn mark_counts(studio: &StudioSummary) -> Vec<(Person, usize)> {
    MARK_ORDER
        .into_iter()
        .filter_map(|mark| {
            let n = studio
                .roles
                .iter()
                .filter(|role| Person::of(role) == mark)
                .count();
            (n > 0).then_some((mark, n))
        })
        .collect()
}

/// The health one studio's rows share — the block header's glyph
/// and the strip line's mark compute it the same way.
pub(crate) fn health_of(
    studio: &StudioSummary,
    stage: bool,
    reachable: bool,
) -> crate::studio::glyphs::Health {
    use crate::studio::glyphs::Health;
    if !reachable || studio.error.is_some() {
        Health::Offline
    } else if !stage {
        Health::StageDown
    } else if studio.ready_to_finish || studio.state == "finished" {
        Health::Ready
    } else {
        Health::Live
    }
}

/// The role groups a block separates with a thin rule, in order.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum RoleGroup {
    Producer,
    Director,
    Writer,
    Actors,
}

impl RoleGroup {
    fn index(self) -> usize {
        match self {
            RoleGroup::Producer => 0,
            RoleGroup::Director => 1,
            RoleGroup::Writer => 2,
            RoleGroup::Actors => 3,
        }
    }
}

/// A role key's group — fixed seats by name, every `actor…` together.
pub(super) fn role_group(role: &str) -> RoleGroup {
    match role {
        "producer" => RoleGroup::Producer,
        "director" => RoleGroup::Director,
        "writer" => RoleGroup::Writer,
        _ => RoleGroup::Actors,
    }
}

/// How many thin rules a block draws across a specific role list.
pub(super) fn divider_count_for(roles: &[&RoleSummary]) -> usize {
    let mut seen = [false; 4];
    for role in roles {
        seen[role_group(&role.role).index()] = true;
    }
    seen.iter()
        .filter(|present| **present)
        .count()
        .saturating_sub(1)
}

/// How many thin rules a block draws: one between each pair of
/// adjacent non-empty role groups, so an empty group leaves no rule.
#[allow(dead_code)]
pub(super) fn divider_count(studio: &StudioSummary) -> usize {
    divider_count_for(&live_roles(studio))
}

/// The studio's non-closed roles in display order: `producer`,
/// `director`, `writer` first when present, then the actors in the
/// summary's order. Closed roles come after (S2 §2).
pub(crate) fn live_roles(studio: &StudioSummary) -> Vec<&RoleSummary> {
    let mut out: Vec<&RoleSummary> = Vec::new();
    for fixed in ["producer", "director", "writer"] {
        out.extend(
            studio
                .roles
                .iter()
                .filter(|r| r.role == fixed && r.state != "closed"),
        );
    }
    out.extend(studio.roles.iter().filter(|r| {
        r.state != "closed" && !matches!(r.role.as_str(), "producer" | "director" | "writer")
    }));
    out
}

/// The studio's closed roles — under the closed fold line (S2 §2).
pub(super) fn closed_roles(studio: &StudioSummary) -> impl Iterator<Item = &RoleSummary> {
    studio.roles.iter().filter(|r| r.state == "closed")
}

/// Whether a live (non-closed) role's row already represents `task`.
pub(super) fn represented(studio: &StudioSummary, task: &TaskSummary) -> bool {
    studio
        .roles
        .iter()
        .any(|r| r.state != "closed" && (r.task.as_deref() == Some(task.id.as_str())))
}

/// The dock's task rows: tasks with no live actor row, minus the
/// terminal states that never draw (`abandoned`, `taken_over`) and,
/// while the done fold is shut, `accepted`/`partially_accepted`.
/// Ledger order is the summary's order.
pub(super) fn task_rows(
    studio: &StudioSummary,
    accepted_open: bool,
) -> impl Iterator<Item = &TaskSummary> {
    let fold = fold_count(studio) >= 2 && !accepted_open;
    studio.tasks.iter().filter(move |t| {
        if matches!(t.status, TaskStatus::Abandoned | TaskStatus::TakenOver)
            || represented(studio, t)
        {
            return false;
        }
        if fold
            && matches!(
                t.status,
                TaskStatus::Accepted | TaskStatus::PartiallyAccepted
            )
        {
            return false;
        }
        true
    })
}

/// The number of accepted tasks behind the fold — the `✓ <n>`
/// line's count; `0` means no fold line at all.
pub(super) fn fold_count(studio: &StudioSummary) -> usize {
    accepted_fold_ids(studio).len()
}

/// A person's line count: itself plus an open `Space` drawer.
pub(super) fn person_height(studio: StudioRef<'_>, role: &RoleSummary, panel: &Panel) -> usize {
    let mut height = 1;
    if panel
        .drawer
        .as_ref()
        .is_some_and(|d| *d == Target::Person(studio.key(), role.role.clone()))
    {
        height += super::row_lines::person_drawer(studio, role).len();
    }
    height
}

/// The task section's line count: task rows plus the fold line and
/// any open drawer under a task row.
pub(super) fn tasks_height(studio: StudioRef<'_>, panel: &Panel, accepted_open: bool) -> usize {
    let mut height = task_rows(studio.studio, accepted_open)
        .map(|t| {
            1 + usize::from(
                panel
                    .drawer
                    .as_ref()
                    .is_some_and(|d| *d == Target::Task(studio.key(), t.id.clone())),
            ) * 3
        })
        .sum::<usize>();
    height += usize::from(fold_count(studio.studio) >= 2);
    height
}

/// The closed section's line count: the `□ <n> closed` fold row when
/// any closed role exists, plus each closed role when open.
pub(super) fn closed_height(studio: StudioRef<'_>, panel: &Panel, closed_open: bool) -> usize {
    if closed_roles(studio.studio).next().is_none() {
        return 0;
    }
    let mut height = 1;
    if closed_open {
        for role in closed_roles(studio.studio) {
            height += person_height(studio, role, panel);
        }
    }
    height
}

/// An expanded block's height without talk rows: top + bottom
/// borders, compose divider + `›` (or reopen line), owed/report/confirm/
/// next/usage lines, live people, role dividers, task section,
/// and closed section.
pub(super) fn block_height(
    studio: StudioRef<'_>,
    panel: &Panel,
    owed: bool,
    accepted_open: bool,
    closed_open: bool,
) -> usize {
    let has_input_divider = !(studio.studio.state == "finished" && !studio.studio.ready_to_finish);
    let report_pending = !studio.studio.needs_upgrade && super::data::report_pending(studio.studio);
    let finish_confirm =
        !studio.studio.needs_upgrade && panel.finish_confirm.as_ref() == Some(&studio.key());
    let next_step = !studio.studio.needs_upgrade && super::data::next_step(studio.studio).is_some();
    let mut height = 3
        + usize::from(has_input_divider)
        + usize::from(owed)
        + usize::from(report_pending)
        + usize::from(finish_confirm)
        + usize::from(next_step)
        + usize::from(studio.studio.usage.is_some())
        + super::draw_design::block_line_count(studio.studio)
        + studio.studio.outward_cards.len();
    let shown = live_roles(studio.studio);
    for role in &shown {
        height += person_height(studio, role, panel);
    }
    height += divider_count_for(&shown);
    height += tasks_height(studio, panel, accepted_open);
    height += closed_height(studio, panel, closed_open);
    height
}

/// How many newest `exchange` lines the selected full block may render
/// under `body_height` without starving roles, tasks, folded blocks,
/// or the `›` input line.
pub(super) fn talk_budgets(
    studios: &[StudioRef<'_>],
    states: &std::collections::BTreeMap<StudioKey, BlockState>,
    panel: &Panel,
    current: Option<&StudioKey>,
    body_height: u16,
    header_rows: usize,
    ctx: &dyn Fn(StudioRef<'_>) -> bool,
) -> std::collections::BTreeMap<StudioKey, usize> {
    let mut budgets = std::collections::BTreeMap::new();
    let Some(sel_key) = selected_studio(studios, panel, current) else {
        return budgets;
    };
    let Some(target) = studios.iter().copied().find(|s| {
        s.key() == sel_key
            && states.get(&sel_key) == Some(&BlockState::Full)
            && !s.studio.exchange.is_empty()
    }) else {
        return budgets;
    };
    if body_height == 0 {
        budgets.insert(sel_key, target.studio.exchange.len());
        return budgets;
    }
    let used = header_rows
        + usize::from(panel.hollywood_hint)
        + usize::from(panel.update.is_some())
        + studios
            .iter()
            .map(|studio| {
                let key = studio.key();
                match states.get(&key).copied().unwrap_or(BlockState::Stacked) {
                    BlockState::Full => block_height(
                        *studio,
                        panel,
                        ctx(*studio),
                        panel.accepted_open(&key),
                        panel.closed_open(&key),
                    ),
                    BlockState::Folded => folded_block_height(studio.studio),
                    BlockState::Compact(_) => {
                        1 + usize::from(panel.finish_confirm.as_ref() == Some(&key))
                    }
                    BlockState::Stacked => 1,
                }
            })
            .sum::<usize>();
    let avail = body_height as usize;
    if used > avail {
        // When non-chat rows exceed a normal-height dock, keep at most
        // the newest 2 turns on the selected studio (`0` at `<= 10`).
        if avail > 10 {
            budgets.insert(sel_key, target.studio.exchange.len().min(2));
        }
        return budgets;
    }
    let slack = avail - used;
    if slack > 0 {
        budgets.insert(sel_key, slack.min(target.studio.exchange.len()));
    }
    budgets
}

/// The collapsed strip's role lines: live roles that are working or
/// demand attention (prompt/asks/needs/stuck/failed/lost/queued).
pub(super) fn strip_roles(studio: &StudioSummary) -> Vec<&RoleSummary> {
    studio
        .roles
        .iter()
        .filter(|r| r.state != "closed" && is_active_or_alert_role(r))
        .collect()
}
