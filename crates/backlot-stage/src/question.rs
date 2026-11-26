//! Interactive `question` attention episodes (R7b, D2): raised once per
//! dialog episode for generic-rule dialogs, structured `kind: "question"`
//! states, or recognised non-permission pickers; routed inside the studio
//! for actors (`notices::reviewer_for`) and `director`/`writer`
//! (`"producer"`), or marked `needs_user` on `producer` and Hollywood;
//! cleared `"dialog closed"` after [`EPISODE_MIN_MS`] out of the episode.

use backlot_model::event::{EventKind, QUESTION};
use backlot_model::state::{Attention, StudioState, actor_task};
use backlot_model::summary::Work;

use crate::act::record;
use crate::cli::Fail;
use crate::daemon::Stage;
use crate::permit::{EPISODE_MIN_MS, episode_due};
use crate::runners::{self, dialog, work};
use crate::sample::{Raise, Watched};
use crate::signals::Signal;
use crate::watch::trust_dialog;

/// Whether the current sample is an interactive `question` episode
/// rather than a trust dialog or a runner permission prompt.
pub(crate) fn in_episode(
    runner: &str,
    combined_work: Work,
    status: Option<&hc_proto::terminal::Status>,
    screen: &str,
    request: Option<&Signal>,
) -> bool {
    if combined_work != Work::Blocked
        || crate::ratelimit::weekly_choice_dialog(screen)
        || trust_dialog(screen)
        || request.is_some()
        || work::recognized_permission(runner, screen)
    {
        return false;
    }
    status.is_some_and(|s| s.state == "blocked" && s.kind.as_deref() == Some("question"))
        || work::recognized_question(runner, screen)
        || dialog::generic_dialog(screen)
}

/// The role's open `question` attention, when it has one.
pub fn open_question<'s>(state: &'s StudioState, role: &str) -> Option<&'s Attention> {
    state
        .open_attention()
        .find(|a| a.role == role && a.reason == QUESTION)
}

/// Whether the current dialog episode already raised a `question`
/// attention at or after `since_ms`.
pub fn episode_raised(state: &StudioState, role: &str, since_ms: u64) -> bool {
    state
        .attention
        .iter()
        .any(|a| a.role == role && a.reason == QUESTION && a.ts_ms >= since_ms)
}

/// Concise one-line question summary stored on `attention.raised` and
/// published on Hollywood's attention detail.
pub(crate) fn dialog_summary(runner: &str, screen: &str) -> String {
    if let Some(detail) = runners::prompt_detail(runner, screen) {
        return detail;
    }
    if let Some(q) = screen
        .lines()
        .rev()
        .find(|l| work::agy_question_line(l))
        .map(|l| l.trim().chars().take(120).collect::<String>())
    {
        return q;
    }
    dialog::question_summary(screen).unwrap_or_else(|| "dialog on screen".to_string())
}

/// Who a `question` attention notices inside the studio:
/// - actor -> task's reviewer (`notices::reviewer_for`, or `"director"`
///   when the task is missing);
/// - `director` / `writer` -> `"producer"`;
/// - `producer` -> `None` (`needs_user: true` surfaces it to the user).
pub fn notice_to(state: &StudioState, attn: &Attention) -> Option<String> {
    if let Some(task) = actor_task(&attn.role) {
        return Some(
            state
                .tasks
                .get(task)
                .map(crate::notices::reviewer_for)
                .unwrap_or_else(|| "director".into()),
        );
    }
    if attn.role == "producer" {
        return None;
    }
    Some("producer".into())
}

/// The `question` notice body, pointing at the saved screen and telling
/// the recipient to nudge/re-prompt or clear/take over the terminal.
pub fn notice_body(attn: &Attention, screen_file: &str) -> String {
    let role = &attn.role;
    let ways = match actor_task(role) {
        Some(id) => format!(
            "\n- `backlot tell {role} --file <file>` to nudge or re-prompt it\n\
             - inspect `{screen_file}` and clear or take over `{role}`'s terminal to answer the dialog\n\
             - `backlot task abandon --task {id} --reason <reason>` or `backlot cast --task {id} --actor <name> --reason <reason>` if it cannot continue\n"
        ),
        None => format!(
            "\n- `backlot tell {role} --file <file>` to nudge or re-prompt it\n\
             - inspect `{screen_file}` and clear or take over `{role}`'s terminal to answer the dialog\n"
        ),
    };
    format!(
        "# {role} question\n\n`{role}` is stopped at an interactive dialog on its own screen — {}. \
         The screen was saved to `{screen_file}` — read it before acting.\n\nWays on:\n{ways}",
        attn.detail
    )
}

impl Stage {
    /// Track a role's `question` dialog episode: raise once after
    /// [`EPISODE_MIN_MS`] with `attention/<seq>.txt`, and clear
    /// `"dialog closed"` once out of the episode for a full window.
    pub(crate) async fn question(
        &self,
        studio: &str,
        w: &Watched<'_>,
        work: Work,
        text: &str,
        request: Option<&Signal>,
        now_ms: u64,
    ) -> Result<(), Fail> {
        let (dir, state, role) = (w.dir, w.state, w.role);
        let pair = (studio.to_string(), role.to_string());
        let ep = in_episode(w.runner, work, w.term.status.as_ref(), text, request);
        let (since, due) = episode_due(
            ep,
            self.q_blocked_since.borrow().get(&pair).copied(),
            now_ms,
        );
        match since {
            Some(since) => {
                self.q_blocked_since
                    .borrow_mut()
                    .insert(pair.clone(), since);
                self.q_unblocked_since.borrow_mut().remove(&pair);
                if due
                    && !episode_raised(state, role, since)
                    && !crate::act::attention_open(state, role, QUESTION)
                {
                    let needs_user = role == "producer";
                    let summary = dialog_summary(w.runner, text);
                    self.raise_attention(
                        Raise {
                            dir,
                            role,
                            reason: QUESTION,
                            text,
                            needs_user,
                            bound: None,
                        },
                        move |seq| {
                            let tail = if needs_user { "needs the user; " } else { "" };
                            format!("{summary}; {tail}screen saved to attention/{seq}.txt")
                        },
                    )
                    .await?;
                    crate::stagelog::stage_log!("{role} stopped at a question dialog");
                }
            }
            None => {
                self.q_blocked_since.borrow_mut().remove(&pair);
                self.q_unblocked_since
                    .borrow_mut()
                    .entry(pair.clone())
                    .or_insert(now_ms);
            }
        }
        let Some(attn) = open_question(state, role) else {
            return Ok(());
        };
        if !ep {
            let gone = self
                .q_unblocked_since
                .borrow()
                .get(&pair)
                .copied()
                .unwrap_or(now_ms);
            if now_ms.saturating_sub(gone) >= EPISODE_MIN_MS {
                record(
                    dir,
                    EventKind::AttentionCleared {
                        target: attn.seq,
                        reason: Some("dialog closed".into()),
                    },
                )?;
            }
        }
        Ok(())
    }
}

#[cfg(test)]
#[path = "question_tests.rs"]
mod tests;
