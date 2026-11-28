//! `backlot guide`: backlot's own documentation, compiled into the
//! binary. Roles are told their docs come from here and never to
//! search the disk for them (field notes 1126 §4) — an agent that
//! globs for `docs/studio/guide.md` in a bound repo finds nothing and
//! raises a permission prompt for the trouble. The whole
//! `docs/studio/` directory travels, one topic per file, so a topic
//! name is the file's stem; `agents` is the agent skill the binary also
//! installs. No studio, ledger or core is touched.

use crate::cli::Fail;

/// One documentation topic: the file's stem and its compiled bytes.
struct Topic {
    name: &'static str,
    body: &'static str,
}

/// `docs/studio/*.md`, compiled in, plus the agent skill. A new file is
/// added here too — the guide test fails until it is, so the binary
/// never silently ships a partial set.
const DOCS: &[Topic] = &[
    // The Agent Skills file, also installed by `backlot skill install`
    // and found by `npx skills add followcat/backlot` — one text, three
    // deliveries.
    Topic {
        name: "agents",
        body: include_str!("../../../skills/backlot/SKILL.md"),
    },
    Topic {
        name: "design",
        body: include_str!("../../../docs/studio/design.md"),
    },
    Topic {
        name: "guide",
        body: include_str!("../../../docs/studio/guide.md"),
    },
    Topic {
        name: "hollywood",
        body: include_str!("../../../docs/studio/hollywood.md"),
    },
    Topic {
        name: "ledger",
        body: include_str!("../../../docs/studio/ledger.md"),
    },
    Topic {
        name: "limits",
        body: include_str!("../../../docs/studio/limits.md"),
    },
    Topic {
        name: "metrics",
        body: include_str!("../../../docs/studio/metrics.md"),
    },
    Topic {
        name: "panel",
        body: include_str!("../../../docs/studio/panel.md"),
    },
    Topic {
        name: "plan",
        body: include_str!("../../../docs/studio/plan.md"),
    },
    Topic {
        name: "progress",
        body: include_str!("../../../docs/studio/progress.md"),
    },
    Topic {
        name: "requests",
        body: include_str!("../../../docs/studio/requests.md"),
    },
    Topic {
        name: "requirements",
        body: include_str!("../../../docs/studio/requirements.md"),
    },
    Topic {
        name: "runners",
        body: include_str!("../../../docs/studio/runners.md"),
    },
    Topic {
        name: "server",
        body: include_str!("../../../docs/studio/server.md"),
    },
    Topic {
        name: "testing",
        body: include_str!("../../../docs/studio/testing.md"),
    },
];

/// Every topic name the binary carries, in file-declaration order.
pub(crate) fn names() -> impl Iterator<Item = &'static str> {
    DOCS.iter().map(|t| t.name)
}

/// The first heading, without its `#` markers — a topic's one-line
/// title in the listing. A file that opens on frontmatter (the agent
/// skill) is titled by the heading after it.
fn title(body: &str) -> &str {
    body.lines()
        .find(|line| line.starts_with('#'))
        .or_else(|| body.lines().next())
        .unwrap_or_default()
        .trim_start_matches('#')
        .trim()
}

/// The lines of `body` that hold `needle`, case-insensitively, with
/// their 1-based numbers.
fn hits<'a>(body: &'a str, needle: &str) -> Vec<(usize, &'a str)> {
    let needle = needle.to_lowercase();
    body.lines()
        .enumerate()
        .filter(|(_, line)| line.to_lowercase().contains(&needle))
        .map(|(i, line)| (i + 1, line))
        .collect()
}

/// One topic's matching lines under a header naming the topic.
fn grep_topic(name: &str, body: &str, needle: &str) -> String {
    let found = hits(body, needle);
    if found.is_empty() {
        return format!("no line in '{name}' matches '{needle}'\n");
    }
    let mut out = format!("{name} — {}\n", title(body));
    for (n, line) in found {
        out.push_str(&format!("{n}: {line}\n"));
    }
    out
}

/// `backlot guide`'s whole behaviour: no topic lists them, a topic
/// prints one, and `--grep` narrows either to matching lines — over
/// every topic when none is named.
pub fn run(topic: Option<&str>, grep: Option<&str>) -> Result<String, Fail> {
    match (topic, grep) {
        (None, None) => {
            let mut out = String::from(
                "backlot's own docs — `backlot guide <topic>` prints one, `--grep <word>` narrows it:\n",
            );
            let mut sorted: Vec<&Topic> = DOCS.iter().collect();
            sorted.sort_by_key(|t| t.name);
            for t in sorted {
                out.push_str(&format!("  {:<12} {}\n", t.name, title(t.body)));
            }
            Ok(out)
        }
        (Some(name), grep) => {
            let topic = DOCS
                .iter()
                .find(|t| t.name == name)
                .ok_or_else(|| unknown(name))?;
            Ok(match grep {
                None => topic.body.to_string(),
                Some(needle) => grep_topic(topic.name, topic.body, needle),
            })
        }
        (None, Some(needle)) => {
            let mut out = String::new();
            for t in DOCS {
                for (n, line) in hits(t.body, needle) {
                    out.push_str(&format!("{}:{n}: {line}\n", t.name));
                }
            }
            if out.is_empty() {
                out = format!("no guide line matches '{needle}'\n");
            }
            Ok(out)
        }
    }
}

/// The refusal a mistyped topic earns, naming what is available.
fn unknown(name: &str) -> Fail {
    let mut names: Vec<&str> = names().collect();
    names.sort_unstable();
    Fail::refused(format!(
        "no such guide topic '{name}' — topics: {}",
        names.join(", ")
    ))
}
