//! Hollywood's own CLI reference, generated from the parser's usage
//! strings. It is written into the `AGENTS.md` the stage hands Hollywood
//! at every start, at a stable place, so the overseer knows every
//! command it may run and its flags without probing `--help` (H2, P5).
//!
//! One table names the commands Hollywood may use — exactly the set its
//! launch pre-allows — and the usage constant each comes from. The line
//! text is sliced out of that constant, so a flag added to or removed
//! from a usage string reaches the reference. The checked-in copy
//! (`prompts/hollywood-cli.md`) is generated, never hand-edited: the
//! drift test regenerates and compares, and a mismatch fails until the
//! file is refreshed with `BLESS=1 cargo test`. The file is the
//! reviewable artifact; the stage writes the same bytes at run time.

use crate::args_hwood::U_HOLLYWOOD;
use crate::args_usage::*;

/// The placeholder in `prompts/hollywood.md` the reference replaces.
pub const MARKER: &str = "{{CLI_REFERENCE}}";

/// One command Hollywood may run: its argv prefix and the usage
/// constant whose text carries its flags.
struct Cmd {
    /// The argv prefix the command starts with (`backlot status`).
    pick: &'static str,
    /// The parser's usage string for it.
    usage: &'static str,
    /// A literal the usage text carries that Hollywood's copy names
    /// concretely — `tell`'s `<role>` is always `producer` for it.
    subst: Option<(&'static str, &'static str)>,
}

/// Hollywood's overseer set — the same commands `runners/*::allow_rules`
/// pre-allows. The read-only `backlot --help` / `backlot help` are the
/// escape for anything not listed.
const COMMANDS: &[Cmd] = &[
    Cmd {
        pick: "backlot status",
        usage: U_STATUS,
        subst: None,
    },
    Cmd {
        pick: "backlot records",
        usage: U_RECORDS,
        subst: None,
    },
    Cmd {
        pick: "backlot tell ",
        usage: U_TELL,
        // Hollywood may write to a producer only (D17a).
        subst: Some(("<role>", "producer")),
    },
    Cmd {
        pick: "backlot studio new",
        usage: U_STUDIO,
        subst: None,
    },
    Cmd {
        pick: "backlot hollywood propose",
        usage: U_HOLLYWOOD,
        subst: None,
    },
    Cmd {
        pick: "backlot hollywood report",
        usage: U_HOLLYWOOD,
        subst: None,
    },
];

/// The argv prefixes the reference documents — the test that Hollywood's
/// launch pre-allow list and the reference agree reads this. Derived
/// from [`COMMANDS`], so the two can never drift apart.
#[cfg(test)]
pub fn picks() -> Vec<&'static str> {
    COMMANDS.iter().map(|c| c.pick).collect()
}

/// The flags a usage line names, in order, deduplicated.
fn flags(line: &str) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for word in line.split_whitespace() {
        let flag = word.trim_start_matches(['[', '(', '{']);
        let Some(stem) = flag.strip_prefix("--") else {
            continue;
        };
        let name = stem
            .split(|c: char| !(c.is_ascii_alphanumeric() || c == '-'))
            .next()
            .unwrap_or_default();
        if name.is_empty() {
            continue;
        }
        let flag = format!("--{name}");
        if !out.contains(&flag) {
            out.push(flag);
        }
    }
    out
}

/// The one line for `cmd`: its usage text flattened and sliced from the
/// argv prefix to the next command (or the `actions:` note), so a
/// multi-command constant yields exactly one command's line.
fn line(cmd: &Cmd) -> String {
    let flat = cmd.usage.split_whitespace().collect::<Vec<_>>().join(" ");
    let start = flat.find(cmd.pick).unwrap_or(0);
    let rest = &flat[start..];
    let after = cmd.pick.len().min(rest.len());
    let end = rest[after..]
        .find(" backlot ")
        .map(|i| i + after)
        .unwrap_or(rest.len());
    let mut line = rest[..end].trim().to_string();
    if let Some((from, to)) = cmd.subst {
        line = line.replace(from, to);
    }
    if let Some(i) = line.find(" actions:") {
        line.truncate(i);
    }
    line
}

/// The generated markdown reference: the read-only escape and one
/// bullet per command with its flags.
pub fn generate() -> String {
    let mut out = String::from(
        "### Command reference\n\n\
         Generated from the CLI's own usage strings. Use it; run `backlot --help`\n\
         only for a command it does not list. Read-only anywhere: `backlot help`,\n\
         `backlot <command> --help`.\n",
    );
    for cmd in COMMANDS {
        let line = line(cmd);
        let flags = flags(&line);
        out.push_str(&format!("\n- `{line}`\n"));
        if !flags.is_empty() {
            out.push_str(&format!("  flags: {}\n", flags.join(" ")));
        }
    }
    out
}

/// Hollywood's `AGENTS.md` body: its prompt with the reference in place
/// of [`MARKER`]. A prompt that lost the marker keeps the reference at
/// the end, so Hollywood always has it.
pub fn agents_md() -> String {
    let prompt = crate::prompts::HOLLYWOOD;
    let reference = generate();
    if prompt.contains(MARKER) {
        prompt.replace(MARKER, &reference)
    } else {
        format!("{prompt}\n{reference}")
    }
}

#[cfg(test)]
#[path = "hwcli_tests.rs"]
mod tests;
