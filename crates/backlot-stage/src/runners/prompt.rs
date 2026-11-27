//! A runner's permission prompt read through its wrapping: the
//! sentence, the command and the picker's option texts move to a new
//! line when the terminal is narrow, so matching and detail reading
//! join the lines first. A prompt the screen cannot name reads as
//! nothing — never guessed at.

/// The screen's lines joined with single spaces — wrapping only
/// moves where the spaces fall.
pub(crate) fn joined(screen: &str) -> String {
    screen.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// The line of `lines` where `phrase` begins in the joined text, even
/// when the phrase itself wraps across two lines.
pub(crate) fn phrase_line(lines: &[&str], phrase: &str) -> Option<usize> {
    let mut text = String::new();
    let mut starts = Vec::with_capacity(lines.len());
    for (i, line) in lines.iter().enumerate() {
        if i > 0 {
            text.push(' ');
        }
        starts.push(text.len());
        text.push_str(line.trim());
    }
    let at = text.find(phrase)?;
    starts.iter().rposition(|&s| s <= at)
}

/// The line that ends a prompt's wrapped text: cmd's `Press [ctrl+e]`
/// hint or the first option cursor of a picker.
fn footer(line: &str) -> bool {
    let t = line.trim_start();
    line.contains("Press [ctrl+e]")
        || t.starts_with("❯ 1.")
        || t.starts_with("› 1.")
        || t.starts_with("→ ")
        || t.starts_with("❭ ")
        || t.starts_with("[y]")
}

/// The wrapped text from the line that begins `phrase` down to the
/// prompt's footer, joined with single spaces.
fn region(lines: &[&str], phrase: &str) -> Option<String> {
    let start = phrase_line(lines, phrase)?;
    let end = lines[start..]
        .iter()
        .position(|l| footer(l))
        .map_or(lines.len(), |i| start + i);
    let text = lines[start..end]
        .iter()
        .map(|l| l.trim())
        .filter(|l| !l.is_empty())
        .collect::<Vec<_>>()
        .join(" ");
    (!text.is_empty()).then_some(text)
}

/// The prompt's own words, independent of the terminal's width: for
/// cmd the whole command after its `needs to execute` / `needs to
/// run` / `make this edit to` sentence (its sentence-ending `.`
/// removed); for every other runner its question line — `Do you want
/// to …`, `Would you like to …`, `Approve tool call …` — unwrapped
/// the same way. `None` when the screen names neither.
pub(crate) fn prompt_detail(runner: &str, screen: &str) -> Option<String> {
    let lines: Vec<&str> = screen.lines().collect();
    if runner == "cmd" {
        for phrase in [
            "needs to execute",
            "needs to run",
            "make this edit to",
            "Do you want to",
        ] {
            let Some(text) = region(&lines, phrase) else {
                continue;
            };
            let Some((_, after)) = text.split_once(phrase) else {
                continue;
            };
            let after = after.trim();
            // cmd's wider sentence keeps a boilerplate tail before the
            // command itself ("needs to execute this shell command:").
            let after = ["this shell command:", "this command:"]
                .iter()
                .find_map(|tail| after.strip_prefix(*tail))
                .map_or(after, str::trim_start);
            let after = after.strip_suffix('.').unwrap_or(after).trim_end();
            if !after.is_empty() {
                return Some(after.to_string());
            }
        }
        return None;
    }
    for phrase in ["Do you want to", "Would you like to", "Approve tool call"] {
        if let Some(text) = region(&lines, phrase) {
            return Some(text);
        }
    }
    None
}
