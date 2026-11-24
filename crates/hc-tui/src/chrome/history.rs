//! Bounded session + summary-backed input history for `›` compose lines.
//! Shared between Call Sheet (`Hollywood.history`) and Studio Dock
//! (`Panel.histories` per `StudioKey`). Consecutive duplicates collapse;
//! recalled entries never mutate stored history until submitted via `Enter`.

use std::collections::VecDeque;

const MAX_ENTRIES: usize = 64;

/// Normalize a single-line history entry to match `TextEditor` storage.
fn normalize(text: &str) -> String {
    text.chars()
        .map(|c| match c {
            '\r' | '\n' | '\t' => ' ',
            other => other,
        })
        .collect::<String>()
        .trim()
        .to_owned()
}

/// Match a normalized summary `user` line against an unconfirmed local send.
/// Supports exact matches, 200-char `…`-truncated exchange snippets, and
/// Studio Dock `/tell <role> <msg>` commands whose recorded exchange omits `/tell <role>`.
fn matches_pending(local: &str, summary_text: &str) -> bool {
    if local == summary_text {
        return true;
    }
    if let Some(rest) = local.strip_prefix("/tell ")
        && let Some((_, msg)) = rest.trim_start().split_once(' ')
    {
        let msg = msg.trim();
        if !msg.is_empty() && matches_truncated(msg, summary_text) {
            return true;
        }
    }
    matches_truncated(local, summary_text)
}

fn matches_truncated(full: &str, snippet: &str) -> bool {
    if full == snippet {
        return true;
    }
    if let Some(prefix) = snippet.strip_suffix('…') {
        return !prefix.is_empty() && full.starts_with(prefix);
    }
    false
}

/// Ordered `›` input history with non-destructive `↑`/`↓` browsing and live draft stash.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub(crate) struct ComposeHistory {
    seeded: Vec<String>,
    pending: VecDeque<String>,
    last_seq: u64,
    view: Vec<String>,
    cursor: Option<usize>,
    stash: Option<String>,
}

impl ComposeHistory {
    /// Merge user messages from an ordered exchange slice `(seq, who, text)`.
    /// Retains previously seeded entries when the summary's window slides,
    /// deduplicates pending local sends once their ledger seq arrives, and resets
    /// if the document ledger seq goes backwards.
    pub(crate) fn sync<'a, I>(&mut self, entries: I)
    where
        I: IntoIterator<Item = (u64, &'a str, &'a str)>,
    {
        let collected: Vec<(u64, &str, &str)> = entries.into_iter().collect();
        let max_seq = collected.iter().map(|(seq, _, _)| *seq).max().unwrap_or(0);
        if max_seq < self.last_seq {
            self.seeded.clear();
            self.pending.clear();
            self.last_seq = 0;
            self.reset_nav();
        }
        let mut changed = max_seq < self.last_seq;
        for (seq, who, raw_text) in collected {
            if seq <= self.last_seq {
                continue;
            }
            if who != "user" {
                continue;
            }
            let text = normalize(raw_text);
            if text.is_empty() {
                continue;
            }
            if let Some(pos) = self.pending.iter().position(|p| matches_pending(p, &text)) {
                for _ in 0..=pos {
                    if let Some(entry) = self.pending.pop_front() {
                        self.seeded.push(entry);
                    }
                }
            } else {
                self.seeded.push(text);
                changed = true;
            }
        }
        if max_seq > self.last_seq {
            self.last_seq = max_seq;
        }
        if self.seeded.len() > MAX_ENTRIES {
            let excess = self.seeded.len() - MAX_ENTRIES;
            self.seeded.drain(..excess);
            changed = true;
        }
        if changed || self.view.is_empty() && (!self.seeded.is_empty() || !self.pending.is_empty())
        {
            self.rebuild_view();
        }
    }

    /// Record an accepted local submit so `↑` recalls it before the next summary.
    pub(crate) fn record_send(&mut self, text: &str) {
        let normalized = normalize(text);
        if normalized.is_empty() {
            self.reset_nav();
            return;
        }
        self.pending.push_back(normalized);
        if self.pending.len() > MAX_ENTRIES {
            self.pending.pop_front();
        }
        self.reset_nav();
        self.rebuild_view();
    }

    /// Step backward (older) in history. First press stashes `current_draft`.
    pub(crate) fn step_up(&mut self, current_draft: &str) -> Option<String> {
        if self.view.is_empty() {
            return None;
        }
        match self.cursor {
            None => {
                self.stash = Some(current_draft.to_owned());
                let idx = self.view.len() - 1;
                self.cursor = Some(idx);
                Some(self.view[idx].clone())
            }
            Some(0) => None,
            Some(idx) => {
                let next = idx - 1;
                self.cursor = Some(next);
                Some(self.view[next].clone())
            }
        }
    }

    /// Step forward (newer) in history, restoring the stashed draft past newest.
    pub(crate) fn step_down(&mut self) -> Option<String> {
        let idx = self.cursor?;
        if idx + 1 < self.view.len() {
            let next = idx + 1;
            self.cursor = Some(next);
            Some(self.view[next].clone())
        } else {
            self.cursor = None;
            Some(self.stash.take().unwrap_or_default())
        }
    }

    /// Exit active history browsing while leaving the caller's editor text untouched.
    pub(crate) fn reset_nav(&mut self) {
        self.cursor = None;
        self.stash = None;
    }

    fn rebuild_view(&mut self) {
        self.view.clear();
        for item in self.seeded.iter().chain(self.pending.iter()) {
            if self.view.last().is_some_and(|prev| prev == item) {
                continue;
            }
            self.view.push(item.clone());
        }
        if let Some(idx) = self.cursor
            && idx >= self.view.len()
        {
            self.reset_nav();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_history_is_noop_for_up_and_down() {
        let mut h = ComposeHistory::default();
        h.sync([(1, "hollywood", "hello")]);
        assert_eq!(h.step_up("wip"), None);
        assert_eq!(h.step_down(), None);
    }

    #[test]
    fn collapses_consecutive_repeats_only() {
        let mut h = ComposeHistory::default();
        h.sync([
            (1, "user", "alpha"),
            (2, "user", "alpha"),
            (3, "user", "beta"),
            (4, "user", "alpha"),
        ]);
        assert_eq!(h.step_up("").as_deref(), Some("alpha"));
        assert_eq!(h.step_up("").as_deref(), Some("beta"));
        assert_eq!(h.step_up("").as_deref(), Some("alpha"));
        assert_eq!(h.step_up(""), None);
    }

    #[test]
    fn pending_send_deduplicates_against_summary_without_doubles() {
        let mut h = ComposeHistory::default();
        h.sync([(1, "user", "older")]);
        h.record_send("local tell");
        h.record_send("/tell alice inspect tests");

        // Available immediately via Up before summary catches up.
        assert_eq!(
            h.step_up("wip").as_deref(),
            Some("/tell alice inspect tests")
        );
        assert_eq!(h.step_up("wip").as_deref(), Some("local tell"));
        assert_eq!(h.step_down().as_deref(), Some("/tell alice inspect tests"));
        assert_eq!(h.step_down().as_deref(), Some("wip"));

        // Summary arrives matching both local sends.
        h.sync([
            (1, "user", "older"),
            (2, "user", "local tell"),
            (3, "user", "inspect tests"),
            (4, "user", "external"),
        ]);
        assert_eq!(h.step_up("").as_deref(), Some("external"));
        assert_eq!(h.step_up("").as_deref(), Some("/tell alice inspect tests"));
        assert_eq!(h.step_up("").as_deref(), Some("local tell"));
        assert_eq!(h.step_up("").as_deref(), Some("older"));
        assert_eq!(h.step_up(""), None);
    }

    #[test]
    fn stashes_and_restores_draft_without_mutating_history() {
        let mut h = ComposeHistory::default();
        h.record_send("first");
        h.record_send("second");
        assert_eq!(h.step_up("unsent").as_deref(), Some("second"));
        assert_eq!(h.step_up("ignored").as_deref(), Some("first"));
        // Caller edits recalled text in its TextEditor; history keeps "first".
        assert_eq!(h.step_down().as_deref(), Some("second"));
        assert_eq!(h.step_down().as_deref(), Some("unsent"));
        assert_eq!(h.step_down(), None);
        assert_eq!(h.step_up("unsent").as_deref(), Some("second"));
        assert_eq!(h.step_up("ignored").as_deref(), Some("first"));
    }

    #[test]
    fn reset_nav_drops_cursor_and_stash() {
        let mut h = ComposeHistory::default();
        h.record_send("one");
        assert_eq!(h.step_up("draft").as_deref(), Some("one"));
        h.reset_nav();
        assert_eq!(h.step_down(), None);
        assert_eq!(h.step_up("edited one").as_deref(), Some("one"));
        assert_eq!(h.step_down().as_deref(), Some("edited one"));
    }
}
