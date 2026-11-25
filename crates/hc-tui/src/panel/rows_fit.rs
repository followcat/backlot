//! The row model's fit half: how tall each block is, which blocks
//! earn an expanded slot inside the body's height, the studio's
//! mark counts and the strip's never-blank role pick. Pure
//! projections over `StudioSummary` + `Panel` — `rows.rs` assembles
//! what these decide.

use backlot_model::state::TaskStatus;
use backlot_model::summary::{RoleSummary, StudioSummary, TaskSummary};

use super::Panel;
use super::data::StudioRef;
use super::rows::*;
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

/// The studio's closed roles — under the task rows (S2 §2).
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
/// while the accepted fold is shut, `accepted`/`partially_accepted`.
/// Ledger order is the summary's order.
pub(super) fn task_rows(
    studio: &StudioSummary,
    accepted_open: bool,
) -> impl Iterator<Item = &TaskSummary> {
    let accepted = studio
        .tasks
        .iter()
        .filter(|t| {
            matches!(
                t.status,
                TaskStatus::Accepted | TaskStatus::PartiallyAccepted
            )
        })
        .count();
    let fold = accepted >= 6 && !accepted_open;
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
        .count()
}

/// A person's line count: itself plus its cue/failed line and an
/// open drawer — as tall as the drawer's own lines, one more when
/// the stage's answer line rides along.
pub(super) fn person_height(studio: StudioRef<'_>, role: &RoleSummary, panel: &Panel) -> usize {
    let mut height = 1 + usize::from(super::rows::person_line_kind(role).is_some());
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
    height += usize::from(fold_count(studio.studio) >= 6);
    height
}

/// An expanded block's height: top + bottom borders, the `›` input
/// line, the owed-step line when present, the armed finish confirm,
/// each person's lines, the task section and the closed section.
pub(super) fn block_height(
    studio: StudioRef<'_>,
    panel: &Panel,
    owed: bool,
    accepted_open: bool,
) -> usize {
    let mut height = 3
        + usize::from(owed)
        + usize::from(super::data::report_pending(studio.studio))
        + usize::from(panel.finish_confirm.as_ref() == Some(&studio.key()))
        + studio.studio.outward_cards.len();
    for role in live_roles(studio.studio) {
        height += person_height(studio, role, panel);
    }
    height += tasks_height(studio, panel, accepted_open);
    for role in closed_roles(studio.studio) {
        height += person_height(studio, role, panel);
    }
    height
}

/// Which studios draw an expanded block: all of them while every
/// expanded block fits the body; past that, the current studio and any
/// block `Space` opened by hand — `collapsed_blocks` wins either way.
pub(super) fn expanded_set(
    studios: &[StudioRef<'_>],
    panel: &Panel,
    current: Option<&StudioKey>,
    body_height: u16,
    ctx: &dyn Fn(StudioRef<'_>) -> bool,
) -> std::collections::BTreeSet<StudioKey> {
    let fits = body_height == 0
        || studios
            .iter()
            .map(|studio| {
                block_height(
                    *studio,
                    panel,
                    ctx(*studio),
                    panel.accepted_open(&studio.key()),
                )
            })
            .sum::<usize>()
            <= body_height as usize;
    studios
        .iter()
        .filter_map(|studio| {
            let key = studio.key();
            let open = if panel.expanded_blocks.contains(&key) {
                true
            } else if panel.collapsed_blocks.contains(&key) {
                false
            } else {
                fits || current == Some(&key)
            };
            open.then_some(key)
        })
        .collect()
}

/// How many newest `exchange` lines each open block may render under
/// `body_height` without starving roles, tasks or the `›` input line.
pub(super) fn talk_budgets(
    studios: &[StudioRef<'_>],
    open: &std::collections::BTreeSet<StudioKey>,
    panel: &Panel,
    current: Option<&StudioKey>,
    body_height: u16,
    header_rows: usize,
    ctx: &dyn Fn(StudioRef<'_>) -> bool,
) -> std::collections::BTreeMap<StudioKey, usize> {
    let mut budgets = std::collections::BTreeMap::new();
    let open_studios: Vec<StudioRef<'_>> = studios
        .iter()
        .copied()
        .filter(|s| open.contains(&s.key()) && !s.studio.exchange.is_empty())
        .collect();
    if open_studios.is_empty() {
        return budgets;
    }
    if body_height == 0 {
        for s in open_studios {
            budgets.insert(s.key(), s.studio.exchange.len());
        }
        return budgets;
    }
    let used = header_rows
        + usize::from(panel.hollywood_hint)
        + studios
            .iter()
            .map(|studio| {
                let key = studio.key();
                if open.contains(&key) {
                    block_height(*studio, panel, ctx(*studio), panel.accepted_open(&key))
                        + usize::from(studio.studio.usage.is_some())
                } else {
                    1
                }
            })
            .sum::<usize>();
    let avail = body_height as usize;
    if used > avail {
        // When the non-chat rows already exceed a normal-height dock,
        // the body scrolls anyway: keep at most the newest 2 turns at
        // the bottom above `›`, and 0 on cramped heights (`<= 10`).
        if avail > 10 {
            for s in open_studios {
                budgets.insert(s.key(), s.studio.exchange.len().min(2));
            }
        }
        return budgets;
    }
    let mut slack = avail - used;
    if slack == 0 {
        return budgets;
    }
    // Distribute slack round-robin across open studios (`current`
    // first so the active studio wins odd spare lines).
    let mut ordered = Vec::with_capacity(open_studios.len());
    if let Some(cur) = current {
        ordered.extend(open_studios.iter().copied().filter(|s| &s.key() == cur));
        ordered.extend(open_studios.iter().copied().filter(|s| &s.key() != cur));
    } else {
        ordered.extend(open_studios);
    }
    while slack > 0 {
        let mut progressed = false;
        for s in &ordered {
            if slack == 0 {
                break;
            }
            let key = s.key();
            let cur = budgets.get(&key).copied().unwrap_or(0);
            if cur < s.studio.exchange.len() {
                budgets.insert(key, cur + 1);
                slack -= 1;
                progressed = true;
            }
        }
        if !progressed {
            break;
        }
    }
    budgets
}

/// The collapsed strip's role lines: live roles that are working or
/// carry an attention — the strip is never blank while the studio
/// has something to say (A4.3).
pub(super) fn strip_roles(studio: &StudioSummary) -> Vec<&RoleSummary> {
    studio
        .roles
        .iter()
        .filter(|r| {
            r.state != "closed"
                && (r.attention.is_some()
                    || r.work == Some(backlot_model::summary::Work::Working)
                    || r.work == Some(backlot_model::summary::Work::Blocked))
        })
        .collect()
}
