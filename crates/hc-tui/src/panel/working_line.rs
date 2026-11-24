//! Fitting the one-line working detail row (`PersonLine::Working`)
//! inside the panel column (D5): `working 47m · "running the parity
//! suite" 12m ago · cargo test 3m`, omitting missing pieces without
//! stray separators and progressive-fitting narrower widths.

use backlot_model::event::age_short;
use backlot_model::summary::RoleSummary;

use crate::chrome::text::{display_width, truncate_end};
use crate::studio::glyphs::Person;

const SEP: &str = " · ";
const MIN_QUOTED_PROGRESS_COLS: usize = 4;

/// Whether `role` shows the D5 working detail row: working state with
/// at least one factual signal (`since_ms`, `progress`, or `running`).
pub(crate) fn show_working_line(role: &RoleSummary) -> bool {
    Person::of(role) == Person::Working
        && (role.since_ms > 0 || role.progress.is_some() || role.running.is_some())
}

/// Format and fit `role`'s working detail row into `max_width` terminal
/// cells: full -> shortened progress inside quotes with `…` -> drop
/// `running` -> shorten/drop progress -> `truncate_end` on head.
pub(crate) fn fit_working_line(role: &RoleSummary, now_ms: u64, max_width: usize) -> String {
    if max_width == 0 {
        return String::new();
    }
    let head = match role.since_ms {
        0 => "working".to_string(),
        since => format!("working {}", age_short(now_ms.saturating_sub(since))),
    };
    let progress = role.progress.as_ref().map(|p| {
        let age = age_short(now_ms.saturating_sub(p.ts_ms));
        (p.text.as_str(), format!("{age} ago"))
    });
    let running = role.running.as_ref().map(|d| match d.since_ms {
        0 => d.cmd.clone(),
        since => format!("{} {}", d.cmd, age_short(now_ms.saturating_sub(since))),
    });

    let full = join_parts(&head, progress.as_ref(), running.as_deref(), None);
    if display_width(&full) <= max_width {
        return full;
    }
    if running.is_some()
        && let Some(fitted) =
            fit_with_progress(&head, progress.as_ref(), running.as_deref(), max_width)
    {
        return fitted;
    }
    let full_no_running = join_parts(&head, progress.as_ref(), None, None);
    if display_width(&full_no_running) <= max_width {
        return full_no_running;
    }
    if let Some(fitted) = fit_with_progress(&head, progress.as_ref(), None, max_width) {
        return fitted;
    }
    truncate_end(&head, max_width)
}

fn join_parts(
    head: &str,
    progress: Option<&(&str, String)>,
    running: Option<&str>,
    quoted_text_override: Option<&str>,
) -> String {
    let mut parts = Vec::with_capacity(3);
    parts.push(head.to_string());
    if let Some((text, age)) = progress {
        let shown = quoted_text_override.unwrap_or(text);
        parts.push(format!("\"{shown}\" {age}"));
    }
    if let Some(cmd) = running {
        parts.push(cmd.to_string());
    }
    parts.join(SEP)
}

fn fit_with_progress(
    head: &str,
    progress: Option<&(&str, String)>,
    running: Option<&str>,
    max_width: usize,
) -> Option<String> {
    let (text, age) = progress?;
    let shell = format!("\"\" {age}");
    let sep_w = display_width(SEP);
    let running_w = running.map_or(0, |d| sep_w + display_width(d));
    let fixed_w = display_width(head) + sep_w + display_width(&shell) + running_w;
    let budget = max_width.checked_sub(fixed_w)?;
    if budget < MIN_QUOTED_PROGRESS_COLS {
        return None;
    }
    let short = truncate_end(text, budget);
    let candidate = join_parts(head, Some(&(text, age.clone())), running, Some(&short));
    (display_width(&candidate) <= max_width).then_some(candidate)
}

#[cfg(test)]
#[path = "tests/working_tests.rs"]
mod tests;
