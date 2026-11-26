//! The readable diff a cast change prints: `backlot studio cast set
//! --dry-run` and a `cast` card's `card show` both show what the file
//! changes — added, removed and changed candidates, limits, checks,
//! timings and the fixed roles' identities.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use crate::cast::CastSettings;
use crate::event::{ActorIdentity, Candidate, CheckDecl};

/// One fixed role's identity before and after.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct IdentityRow {
    pub role: String,
    pub was: ActorIdentity,
    pub now: ActorIdentity,
}

/// One candidate that appeared, left or changed (`None` on the side it
/// is not on).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CandidateRow {
    pub name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub was: Option<Candidate>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub now: Option<Candidate>,
}

/// One runner's limit before and after.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LimitRow {
    pub runner: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub was: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub now: Option<u32>,
}

/// One declared check before and after.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CheckRow {
    pub name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub was: Option<CheckDecl>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub now: Option<CheckDecl>,
}

/// A scalar setting before and after, as it prints.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ValueRow {
    pub field: String,
    pub was: String,
    pub now: String,
}

/// Everything a cast file changes, in a stable order.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct CastDiff {
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub roles: Vec<IdentityRow>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub candidates: Vec<CandidateRow>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub limits: Vec<LimitRow>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub checks: Vec<CheckRow>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub fields: Vec<ValueRow>,
}

impl CastDiff {
    pub fn is_empty(&self) -> bool {
        self.roles.is_empty()
            && self.candidates.is_empty()
            && self.limits.is_empty()
            && self.checks.is_empty()
            && self.fields.is_empty()
    }

    /// The fixed roles that moved, with the new identity — the ones
    /// carried out as `role.recast` alongside the settings event, so
    /// the stage swaps a live terminal the way a recast makes it.
    pub fn moved_roles(&self) -> impl Iterator<Item = (&str, &ActorIdentity)> {
        self.roles.iter().map(|r| (r.role.as_str(), &r.now))
    }

    /// The diff as a person reads it, one change per line.
    pub fn lines(&self) -> Vec<String> {
        let mut out = Vec::new();
        for r in &self.roles {
            out.push(format!("{}: {} → {}", r.role, label(&r.was), label(&r.now)));
        }
        for c in &self.candidates {
            match (&c.was, &c.now) {
                (None, Some(n)) => {
                    out.push(format!("candidate + {} ({})", n.name, label(&n.actor())))
                }
                (Some(o), None) => {
                    out.push(format!("candidate - {} ({})", o.name, label(&o.actor())))
                }
                (Some(o), Some(n)) => out.push(format!(
                    "candidate ~ {}: {} → {}",
                    o.name,
                    label(&o.actor()),
                    label(&n.actor())
                )),
                (None, None) => {}
            }
        }
        for l in &self.limits {
            match (l.was, l.now) {
                (None, Some(n)) => out.push(format!("limits + {} {n}", l.runner)),
                (Some(o), None) => out.push(format!("limits - {} {o}", l.runner)),
                (Some(o), Some(n)) => out.push(format!("limits {} {o} → {n}", l.runner)),
                (None, None) => {}
            }
        }
        for c in &self.checks {
            match (&c.was, &c.now) {
                (None, Some(n)) => out.push(format!("check + {}", check_label(n))),
                (Some(o), None) => out.push(format!("check - {}", check_label(o))),
                (Some(o), Some(n)) => {
                    out.push(format!("check ~ {} → {}", check_label(o), check_label(n)))
                }
                (None, None) => {}
            }
        }
        for f in &self.fields {
            out.push(format!("{} {} → {}", f.field, f.was, f.now));
        }
        out
    }
}

/// `<runner>/<model>[/<effort>] (<profile>)`.
pub fn label(actor: &ActorIdentity) -> String {
    format!(
        "{} ({})",
        crate::recast::identity_text(actor),
        actor.permission_profile
    )
}

fn check_label(c: &CheckDecl) -> String {
    let run = if c.run.chars().any(char::is_whitespace) {
        format!("`{}`", c.run)
    } else {
        c.run.clone()
    };
    format!("{} {run} ({}s)", c.name, c.timeout_s)
}

/// The diff from `old` to `new`, in role, candidate, limit, check,
/// scalar order.
pub fn diff(old: &CastSettings, new: &CastSettings) -> CastDiff {
    let mut d = CastDiff::default();
    for (role, was, now) in [
        ("producer", &old.roles.producer, &new.roles.producer),
        ("director", &old.roles.director, &new.roles.director),
        ("writer", &old.roles.writer, &new.roles.writer),
    ] {
        if was != now {
            d.roles.push(IdentityRow {
                role: role.into(),
                was: was.clone(),
                now: now.clone(),
            });
        }
    }
    d.candidates = candidates(old, new);
    d.limits = limits(old, new);
    d.checks = checks(old, new);
    d.fields = fields(old, new);
    d
}

fn candidates(old: &CastSettings, new: &CastSettings) -> Vec<CandidateRow> {
    let was: BTreeMap<&str, &Candidate> = old
        .candidates
        .iter()
        .map(|c| (c.name.as_str(), c))
        .collect();
    let now: BTreeMap<&str, &Candidate> = new
        .candidates
        .iter()
        .map(|c| (c.name.as_str(), c))
        .collect();
    let mut names: Vec<&str> = was.keys().chain(now.keys()).copied().collect();
    names.sort_unstable();
    names.dedup();
    names
        .into_iter()
        .filter(|name| was.get(name) != now.get(name))
        .map(|name| CandidateRow {
            name: name.into(),
            was: was.get(name).map(|c| (*c).clone()),
            now: now.get(name).map(|c| (*c).clone()),
        })
        .collect()
}

fn limits(old: &CastSettings, new: &CastSettings) -> Vec<LimitRow> {
    let mut runners: Vec<&str> = old
        .limits
        .keys()
        .chain(new.limits.keys())
        .map(String::as_str)
        .collect();
    runners.sort_unstable();
    runners.dedup();
    runners
        .into_iter()
        .filter_map(|runner| {
            let (was, now) = (
                old.limits.get(runner).copied(),
                new.limits.get(runner).copied(),
            );
            (was != now).then(|| LimitRow {
                runner: runner.into(),
                was,
                now,
            })
        })
        .collect()
}

fn checks(old: &CastSettings, new: &CastSettings) -> Vec<CheckRow> {
    let was: BTreeMap<&str, &CheckDecl> = old.checks.iter().map(|c| (c.name.as_str(), c)).collect();
    let now: BTreeMap<&str, &CheckDecl> = new.checks.iter().map(|c| (c.name.as_str(), c)).collect();
    let mut names: Vec<&str> = was.keys().chain(now.keys()).copied().collect();
    names.sort_unstable();
    names.dedup();
    names
        .into_iter()
        .filter(|name| was.get(name) != now.get(name))
        .map(|name| CheckRow {
            name: name.into(),
            was: was.get(name).map(|c| (*c).clone()),
            now: now.get(name).map(|c| (*c).clone()),
        })
        .collect()
}

fn fields(old: &CastSettings, new: &CastSettings) -> Vec<ValueRow> {
    let mut out = Vec::new();
    let mut row = |field: &str, was: String, now: String| {
        if was != now {
            out.push(ValueRow {
                field: field.into(),
                was,
                now,
            });
        }
    };
    row(
        "rework_cap",
        old.rework_cap.to_string(),
        new.rework_cap.to_string(),
    );
    row(
        "stall_limit_s",
        old.stall_limit_s.to_string(),
        new.stall_limit_s.to_string(),
    );
    row(
        "nudge_after_s",
        old.nudge_after_s.to_string(),
        new.nudge_after_s.to_string(),
    );
    row(
        "escalate_after_s",
        old.escalate_after_s.to_string(),
        new.escalate_after_s.to_string(),
    );
    let budget = |s: &CastSettings| match s.task_budget_s {
        Some(v) => v.to_string(),
        None => "none".into(),
    };
    row("task_budget_s", budget(old), budget(new));
    row(
        "max_actors",
        old.max_actors.to_string(),
        new.max_actors.to_string(),
    );
    row(
        "design_gate",
        old.design_gate.to_string(),
        new.design_gate.to_string(),
    );
    row("check_env", list(&old.check_env), list(&new.check_env));
    row(
        "push.protected_branches",
        list(&old.push.protected_branches),
        list(&new.push.protected_branches),
    );
    row(
        "push.protect_tags",
        old.push.protect_tags.to_string(),
        new.push.protect_tags.to_string(),
    );
    row(
        "push.protect_default_branch",
        old.push.protect_default_branch.to_string(),
        new.push.protect_default_branch.to_string(),
    );
    out
}

fn list(items: &[String]) -> String {
    if items.is_empty() {
        "(none)".into()
    } else {
        items.join(", ")
    }
}
