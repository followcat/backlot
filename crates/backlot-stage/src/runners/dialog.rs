//! Generic fallback dialog detection (D2): catches prompts and pickers
//! whose runner-specific wording was missed or changed, without
//! firing on idle composers, transcript prose, fenced code blocks, or
//! bare `esc to cancel` working bars.

const TAIL_LINES: usize = 12;

#[derive(Clone, Copy)]
struct Line<'a> {
    text: &'a str,
    in_fence: bool,
}

/// Whether `screen` appears to be waiting on an interactive dialog
/// within the last 12 non-blank lines: an option-cursor line (`❯`,
/// `>`, `→`, or `›` followed by a space and text) paired either with a
/// contiguous sibling option at the option text column or with a
/// dialog footer/question marker.
pub(crate) fn generic_dialog(screen: &str) -> bool {
    let tail = tail_window(screen);
    if tail.is_empty() || has_bottom_composer(&tail) {
        return false;
    }
    for (i, line) in tail.iter().enumerate() {
        if line.in_fence || is_blockquote(&tail, i) {
            continue;
        }
        let Some((indent, opt_col)) = option_cursor(line.text) else {
            continue;
        };
        if has_sibling_option(&tail, i, indent, opt_col) || has_dialog_footer(&tail, i) {
            return true;
        }
    }
    false
}

/// Last 12 non-blank lines paired with fenced code-block membership
/// tracked from the top of `screen` so an opening fence above the
/// window still masks lines inside it.
fn tail_window(screen: &str) -> Vec<Line<'_>> {
    let mut lines = Vec::new();
    let mut in_fence = false;
    for raw in screen.lines() {
        let trimmed = raw.trim();
        if trimmed.starts_with("```") {
            in_fence = !in_fence;
            lines.push(Line {
                text: raw,
                in_fence: true,
            });
            continue;
        }
        if !trimmed.is_empty() {
            lines.push(Line {
                text: raw,
                in_fence,
            });
        }
    }
    if lines.len() > TAIL_LINES {
        lines.drain(..lines.len() - TAIL_LINES);
    }
    lines
}

/// Full-width horizontal divider or composer border rule (`─`, `━`,
/// `═`, `▄`, `▀`, `╌`, or long `-` runs).
fn is_composer_rule(l: &str) -> bool {
    let t = l.trim();
    let count = t.chars().count();
    (count >= 3
        && t.chars()
            .all(|c| matches!(c, '─' | '━' | '═' | '▄' | '▀' | '╌')))
        || (count >= 10 && t.chars().all(|c| c == '-'))
}

/// True when `tail` ends with a runner's bottom composer box (an input
/// row between two horizontal border rules, plus up to 3 trailing
/// status/shortcut lines) that does not hold multi-option dialog choices
/// or a dialog action footer.
fn has_bottom_composer(tail: &[Line<'_>]) -> bool {
    let min_close = tail.len().saturating_sub(4);
    let Some(close) = (min_close..tail.len())
        .rfind(|&idx| !tail[idx].in_fence && is_composer_rule(tail[idx].text))
    else {
        return false;
    };
    if tail[close + 1..]
        .iter()
        .any(|l| !l.in_fence && is_action_footer(l.text))
    {
        return false;
    }
    let min_open = close.saturating_sub(5);
    for open in (min_open..close.saturating_sub(1)).rev() {
        if tail[open].in_fence || !is_composer_rule(tail[open].text) {
            continue;
        }
        let inner = &tail[open + 1..close];
        if inner
            .iter()
            .any(|l| l.in_fence || is_composer_rule(l.text) || has_any_dialog_marker(l.text))
        {
            continue;
        }
        let has_options = inner.iter().enumerate().any(|(j, l)| {
            option_cursor(l.text)
                .is_some_and(|(ind, opt_col)| has_sibling_option(inner, j, ind, opt_col))
        });
        if !has_options {
            return true;
        }
    }
    false
}

fn leading_spaces(l: &str) -> usize {
    l.bytes().take_while(|&b| b == b' ').count()
}

/// Parse an option cursor (`❯`, `>`, `→`, or `›` + at least one space +
/// alphanumeric text), returning `(cursor_indent, option_text_col)`.
fn option_cursor(l: &str) -> Option<(usize, usize)> {
    let indent = leading_spaces(l);
    let after = &l[indent..];
    let mut chars = after.chars();
    let marker = chars.next()?;
    if !matches!(marker, '❯' | '>' | '→' | '›') {
        return None;
    }
    let rest = chars.as_str();
    let spaces = leading_spaces(rest);
    if spaces == 0 {
        return None;
    }
    let text = rest[spaces..].trim();
    if text.is_empty() || !text.chars().any(char::is_alphanumeric) {
        return None;
    }
    Some((indent, indent + 1 + spaces))
}

/// Reject consecutive markdown blockquote lines (`> ...` on adjacent
/// rows at the same indent), which pickers never draw.
fn is_blockquote(tail: &[Line<'_>], i: usize) -> bool {
    let cur = &tail[i].text;
    let indent = leading_spaces(cur);
    if !cur[indent..].starts_with("> ") {
        return false;
    }
    let matches_gt = |l: &Line<'_>| {
        !l.in_fence && leading_spaces(l.text) == indent && l.text[indent..].starts_with("> ")
    };
    (i > 0 && matches_gt(&tail[i - 1])) || tail.get(i + 1).is_some_and(matches_gt)
}

/// Contiguous sibling option directly under cursor line `i`: same text
/// column `opt_col` (allowing wrapped option continuations at indent >
/// cursor indent), no intervening non-option line at or left of cursor
/// indent, rule, or bullet prefix.
fn has_sibling_option(active: &[Line<'_>], i: usize, indent: usize, opt_col: usize) -> bool {
    for line in &active[i + 1..] {
        if line.in_fence || is_composer_rule(line.text) {
            break;
        }
        if option_cursor(line.text).is_some_and(|(ind, _)| ind == indent) {
            break;
        }
        let lead = leading_spaces(line.text);
        let trimmed = line.text[lead..].trim();
        if lead == opt_col {
            if is_option_text(trimmed) {
                return true;
            }
            if lead <= indent {
                break;
            }
        } else if lead <= indent {
            break;
        }
    }
    false
}

fn is_option_text(trimmed: &str) -> bool {
    let Some(first) = trimmed.chars().next() else {
        return false;
    };
    (first.is_alphanumeric() || matches!(first, '[' | '('))
        && !matches!(
            first,
            '•' | '◦' | '▪' | '▫' | '●' | '▶' | '▸' | '─' | '│' | '├' | '└'
        )
}

fn has_dialog_footer(tail: &[Line<'_>], cursor_idx: usize) -> bool {
    tail[cursor_idx + 1..]
        .iter()
        .any(|l| !l.in_fence && is_action_footer(l.text))
        || has_question_phrase_near(tail, cursor_idx)
}

fn has_question_phrase_near(tail: &[Line<'_>], cursor_idx: usize) -> bool {
    for (k, line) in tail.iter().enumerate() {
        if k == cursor_idx || line.in_fence || !is_prompt_suffix(line.text) {
            continue;
        }
        let (lo, hi) = if k < cursor_idx {
            (k + 1, cursor_idx)
        } else {
            (cursor_idx + 1, k)
        };
        if !tail[lo..hi]
            .iter()
            .any(|between| between.in_fence || is_composer_rule(between.text))
        {
            return true;
        }
    }
    false
}

fn has_any_dialog_marker(l: &str) -> bool {
    is_action_footer(l) || is_prompt_suffix(l)
}

fn is_action_footer(l: &str) -> bool {
    let lower = l.to_ascii_lowercase();
    lower.contains("enter to select")
        || lower.contains("enter select")
        || lower.contains("enter confirm")
        || (l.contains("↑/↓") && lower.contains("navigate"))
}

fn is_prompt_suffix(l: &str) -> bool {
    let lower = l.to_ascii_lowercase();
    l.contains("Do you want to") || lower.contains("(y/n)") || lower.contains("[y/n]")
}
