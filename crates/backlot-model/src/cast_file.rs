//! The cast as a cast file — what `backlot studio cast show` prints.
//! The text is the shape `studio new --cast` parses, so showing a cast
//! and setting it back is one round trip.

use std::fmt::Write as _;

use crate::cast::CastSettings;
use crate::event::ActorIdentity;

/// The cast file's text, in the order the guide's example uses:
/// scalars, `[limits]`, the three roles, the candidates, the checks,
/// `[push]`.
pub fn to_toml(s: &CastSettings) -> String {
    let mut out = String::new();
    let _ = writeln!(out, "rework_cap = {}", s.rework_cap);
    let _ = writeln!(out, "stall_limit_s = {}", s.stall_limit_s);
    let _ = writeln!(out, "nudge_after_s = {}", s.nudge_after_s);
    let _ = writeln!(out, "escalate_after_s = {}", s.escalate_after_s);
    let _ = writeln!(out, "max_actors = {}", s.max_actors);
    if let Some(budget) = s.task_budget_s {
        let _ = writeln!(out, "task_budget_s = {budget}");
    }
    if !s.check_env.is_empty() {
        let _ = writeln!(out, "check_env = {}", array(&s.check_env));
    }
    if !s.limits.is_empty() {
        out.push_str("\n[limits]\n");
        for (runner, cap) in &s.limits {
            let _ = writeln!(out, "{runner} = {cap}");
        }
    }
    for (name, ident) in [
        ("producer", &s.roles.producer),
        ("director", &s.roles.director),
        ("writer", &s.roles.writer),
    ] {
        let _ = write!(out, "\n[{name}]\n");
        actor(&mut out, ident);
    }
    for c in &s.candidates {
        out.push_str("\n[[candidates]]\n");
        let _ = writeln!(out, "name = {}", string(&c.name));
        actor(&mut out, &c.actor());
    }
    for c in &s.checks {
        out.push_str("\n[[checks]]\n");
        let _ = writeln!(out, "name = {}", string(&c.name));
        let _ = writeln!(out, "run = {}", string(&c.run));
        let _ = writeln!(out, "timeout_s = {}", c.timeout_s);
    }
    out.push_str("\n[push]\n");
    let _ = writeln!(
        out,
        "protected_branches = {}",
        array(&s.push.protected_branches)
    );
    let _ = writeln!(out, "protect_tags = {}", s.push.protect_tags);
    let _ = writeln!(
        out,
        "protect_default_branch = {}",
        s.push.protect_default_branch
    );
    out
}

fn actor(out: &mut String, a: &ActorIdentity) {
    let _ = writeln!(out, "runner = {}", string(&a.runner));
    let _ = writeln!(out, "model = {}", string(&a.model));
    if let Some(effort) = a.effort.as_deref() {
        let _ = writeln!(out, "effort = {}", string(effort));
    }
    let _ = writeln!(
        out,
        "permission_profile = {}",
        string(&a.permission_profile)
    );
}

fn array(items: &[String]) -> String {
    let parts: Vec<String> = items.iter().map(|s| string(s)).collect();
    format!("[{}]", parts.join(", "))
}

/// A TOML basic string. Cast values are slugs and model ids; the
/// escapes cover the rest without pretending to be a full writer.
fn string(s: &str) -> String {
    let mut out = String::from("\"");
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if c.is_control() => {
                let _ = write!(out, "\\u{:04X}", c as u32);
            }
            c => out.push(c),
        }
    }
    out.push('"');
    out
}
