//! Builds the sidebar's rows and counters from a `watch::View`. Pure
//! data in, pure rows out — every string here is already sanitized, so
//! the renderer never sees raw API text. Semantics: docs/sidebar.md.

use super::rows::{Counters, Row, RowId, Summary, Tone, ViewMode};
use super::status::{self, TermStatus, Word};
use super::text;
use hc_proto::terminal::Terminal;
use hc_proto::watch::{Link, View};
use std::collections::BTreeMap;

/// A terminal plus what the model computed about it.
struct Entry<'t> {
    address: &'t str,
    term: &'t Terminal,
    status: TermStatus,
}

/// One machine group: heading facts plus its terminals.
struct MachineGroup<'t> {
    key: String,
    name: String,
    online: bool,
    state: String,
    note: String,
    terms: Vec<Entry<'t>>,
}

/// The machine part of a localized address: `host-b` of `host-b/t7`,
/// `None` for a bare local `t7`. Matched verbatim — never re-prefixed.
fn machine_of(address: &str) -> Option<&str> {
    address.split_once('/').map(|(prefix, _)| prefix)
}

/// Terminal number for ordering: `t7` → 7; anything else sorts last.
fn term_num(address: &str) -> u64 {
    let local = address.rsplit('/').next().unwrap_or(address);
    local
        .strip_prefix('t')
        .and_then(|n| n.parse().ok())
        .unwrap_or(u64::MAX)
}

/// A terminal's display name: `name`, else `title`, else the address.
fn term_name(term: &Terminal) -> String {
    term.name
        .as_deref()
        .filter(|n| !n.is_empty())
        .or(if term.title.is_empty() {
            None
        } else {
            Some(term.title.as_str())
        })
        .unwrap_or(&term.address)
        .to_string()
}

/// `worker of <parent>` — parent addresses are already localized by
/// core, so a remote worker can name a bare local `tN`; matched
/// verbatim against the inventory for its name when known, else shown
/// as the raw address. Unknown cross-core parents stay visible.
fn worker_note(term: &Terminal, view: &View) -> String {
    let Some(parent) = &term.parent else {
        return String::new();
    };
    let who = match view.inventory.get(parent) {
        Some(parent_term) => {
            let name = term_name(parent_term);
            if name == *parent {
                text::clean(parent)
            } else {
                format!("{} {}", text::clean(parent), text::clean(&name))
            }
        }
        None => text::clean(parent),
    };
    format!("worker of {who}")
}

/// This core's heading state comes from the watcher's subscription
/// link; remote machines keep `machine.list`'s own words.
fn local_state(view: &View) -> (bool, String, String) {
    match &view.link {
        Link::Online => (true, "connected".into(), String::new()),
        Link::Static { reason } => (true, "static".into(), text::clean(reason)),
        Link::Connecting { .. } => (false, "connecting".into(), String::new()),
        Link::Offline { error, .. } => (false, "offline".into(), text::clean(error)),
        Link::Stopped => (false, "stopped".into(), String::new()),
    }
}

/// Group the inventory's terminals under machines: local first, then
/// sorted names; a prefix `machine.list` does not describe still forms
/// its own group — stray terminals are never dropped.
fn groups<'t>(view: &'t View, local_name: &str) -> Vec<MachineGroup<'t>> {
    let mut by_machine: BTreeMap<String, Vec<Entry<'t>>> = BTreeMap::new();
    for (address, term) in view.inventory.terminals() {
        let key = machine_of(address).unwrap_or(local_name).to_string();
        by_machine.entry(key).or_default().push(Entry {
            address,
            term,
            status: status::status_of(term),
        });
    }

    let machine_states: BTreeMap<&str, _> = view
        .machines
        .as_deref()
        .unwrap_or_default()
        .iter()
        .map(|m| (m.name.as_str(), m))
        .collect();

    let (online, state, note) = local_state(view);
    let local_online = online && !view.inventory.is_offline();
    let mut out = vec![MachineGroup {
        key: local_name.to_string(),
        name: local_name.to_string(),
        online: local_online,
        state,
        note,
        terms: by_machine.remove(local_name).unwrap_or_default(),
    }];
    // Freshness has two sources. The subscription link plus the
    // watcher's own offline flag say whether terminal rows still flow:
    // with it dead, every remote row is a cached view — dimmed, marked
    // `·stale`, and counted offline. `machine.list` refreshes over rpc;
    // with only rpc dead the machine's own state word is stale but its
    // terminals still stream — hint on the heading, no dimming or
    // offline counting.
    let stream_live =
        matches!(view.link, Link::Online | Link::Static { .. }) && !view.inventory.is_offline();
    let machines_fresh = matches!(view.rpc, Link::Online | Link::Static { .. });
    let state_of = |state: String| {
        if stream_live && machines_fresh {
            state
        } else {
            format!("{state}·stale")
        }
    };

    // Every machine `machine.list` reports stays listed, connected or
    // not — even with an empty terminal set.
    for m in view.machines.as_deref().unwrap_or_default() {
        if m.name == local_name {
            continue; // this core's own name: already the local group
        }
        let terms = by_machine.remove(&m.name).unwrap_or_default();
        out.push(MachineGroup {
            key: m.name.clone(),
            name: m.name.clone(),
            online: stream_live && m.state == "connected",
            state: state_of(text::clean(&m.state)),
            note: m.why.as_deref().map(text::clean).unwrap_or_default(),
            terms,
        });
    }
    for (key, terms) in by_machine {
        let (online, state, note) = match machine_states.get(key.as_str()) {
            Some(m) => (
                stream_live && m.state == "connected",
                state_of(text::clean(&m.state)),
                m.why.as_deref().map(text::clean).unwrap_or_default(),
            ),
            None => (false, "unlisted".into(), String::new()),
        };
        out.push(MachineGroup {
            name: key.clone(),
            key,
            online,
            state,
            note,
            terms,
        });
    }
    // Machines beyond the local group sort by name; `machine.list`
    // entries and stray prefixes merge deterministically.
    out[1..].sort_by(|a, b| a.key.cmp(&b.key));
    for group in &mut out {
        group
            .terms
            .sort_by_key(|e| (term_num(e.address), e.address));
    }
    out
}

/// A terminal's space: the `herdr.space` label when present, else the
/// machine's `cwd`. The key keeps the full cwd so two directories
/// sharing a basename stay distinct spaces.
fn space_of(term: &Terminal) -> (String, String) {
    if let Some(space) = term.labels.get("herdr.space") {
        return (format!("space:{space}"), text::clean(space));
    }
    let cwd = term.cwd.as_str();
    let base = cwd
        .trim_end_matches('/')
        .rsplit('/')
        .next()
        .filter(|b| !b.is_empty())
        .unwrap_or(cwd);
    (format!("cwd:{cwd}"), text::clean(base))
}

/// Tone for a terminal row: the effective word, or lifecycle for
/// unreported/lost/finished terminals.
fn tone_of(status: &TermStatus) -> Tone {
    match (&status.life, status.effective.as_ref().map(|e| &e.word)) {
        (status::Life::Finished, _) | (status::Life::Other(_), _) => Tone::Muted,
        (status::Life::Lost, _) => Tone::Lost,
        (status::Life::Pending, _) | (status::Life::Live, None) => Tone::Quiet,
        (status::Life::Live, Some(Word::Blocked { .. })) => Tone::Blocked,
        (status::Life::Live, Some(Word::Error)) => Tone::Error,
        (status::Life::Live, Some(Word::Working { .. })) => Tone::Working,
        (status::Life::Live, Some(Word::Done)) => Tone::Done,
        (status::Life::Live, Some(_)) => Tone::Quiet,
    }
}

fn term_row(entry: &Entry<'_>, view: &View, depth: u8, dim: bool) -> Row {
    let name = text::clean(&term_name(entry.term));
    // Under its own machine heading the `alias/` prefix is redundant;
    // the local part keeps `t7` visible under even a long alias. In the
    // agents list (depth 0) the qualified address is the machine info.
    let address = text::clean(if depth > 0 {
        entry.address.rsplit('/').next().unwrap_or(entry.address)
    } else {
        entry.address
    });
    let text = if name == address || name.as_str() == entry.address {
        address
    } else {
        format!("{address} {name}")
    };
    Row {
        id: RowId::Terminal(entry.address.to_string()),
        depth,
        text,
        note: worker_note(entry.term, view),
        badge: entry.status.badge.clone(),
        tone: tone_of(&entry.status),
        dim,
    }
}

/// Counters over *live* agents only, counted by effective word; each
/// terminal counts once, workers included. A finished, lost or
/// still-starting program's last report is not a live agent.
fn counters(groups: &[MachineGroup<'_>]) -> Counters {
    let mut c = Counters::default();
    for entry in groups.iter().flat_map(|g| &g.terms) {
        if !entry.status.agent || entry.status.life != status::Life::Live {
            continue;
        }
        match entry.status.effective.as_ref().map(|e| &e.word) {
            Some(Word::Blocked { .. }) => c.your_turn += 1,
            Some(Word::Working { .. }) => c.working += 1,
            Some(Word::Done) => c.ready += 1,
            _ => {}
        }
    }
    c
}

/// The sidebar rows for one view snapshot and mode, plus counters and
/// the compact-line summary.
pub fn build(view: &View, mode: ViewMode) -> (Vec<Row>, Counters, Summary) {
    let local_name = view
        .core
        .as_ref()
        .map(|c| c.name.as_str())
        .filter(|n| !n.is_empty())
        .unwrap_or("local");
    let groups = groups(view, local_name);
    let counters = counters(&groups);
    let summary = Summary {
        machines: groups.len(),
        offline: groups.iter().filter(|g| !g.online).count(),
    };

    let mut rows = Vec::new();
    match mode {
        ViewMode::Spaces => {
            for group in &groups {
                rows.push(Row {
                    id: RowId::Machine(group.key.clone()),
                    depth: 0,
                    text: text::clean(&group.name),
                    note: group.note.clone(),
                    badge: text::clean(&group.state),
                    tone: if group.online {
                        Tone::Heading
                    } else {
                        Tone::Offline
                    },
                    dim: !group.online,
                });
                let mut spaces: BTreeMap<String, (String, Vec<&Entry<'_>>)> = BTreeMap::new();
                for entry in &group.terms {
                    let (key, name) = space_of(entry.term);
                    spaces
                        .entry(key)
                        .or_insert_with(|| (name, Vec::new()))
                        .1
                        .push(entry);
                }
                for (key, (name, terms)) in spaces {
                    let worst = terms
                        .iter()
                        .max_by_key(|e| e.status.urgency)
                        .map(|e| &e.status);
                    rows.push(Row {
                        id: RowId::Space {
                            machine: group.key.clone(),
                            key: key.clone(),
                        },
                        depth: 1,
                        text: format!("{name} ({})", terms.len()),
                        note: String::new(),
                        badge: worst.map(|s| s.badge.clone()).unwrap_or_default(),
                        tone: worst.map(tone_of).unwrap_or(Tone::Muted),
                        dim: !group.online,
                    });
                    for entry in terms {
                        rows.push(term_row(entry, view, 2, !group.online));
                    }
                }
            }
        }
        ViewMode::Agents => {
            let mut agents: Vec<&Entry<'_>> = groups
                .iter()
                .flat_map(|g| g.terms.iter())
                .filter(|e| e.status.agent)
                .collect();
            agents.sort_by(|a, b| {
                b.status
                    .urgency
                    .cmp(&a.status.urgency)
                    .then_with(|| {
                        machine_of(a.address)
                            .unwrap_or("")
                            .cmp(machine_of(b.address).unwrap_or(""))
                    })
                    .then_with(|| {
                        (term_num(a.address), a.address).cmp(&(term_num(b.address), b.address))
                    })
            });
            for entry in agents {
                let machine = machine_of(entry.address).unwrap_or(local_name);
                let dim = groups
                    .iter()
                    .find(|g| g.key == machine)
                    .is_some_and(|g| !g.online);
                rows.push(term_row(entry, view, 0, dim));
            }
        }
    }
    (rows, counters, summary)
}
