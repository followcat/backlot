// Derived from herdr-app src/mirror.rs and src/layout.rs (Apache-2.0).
use super::{Space, Tab, Tree};
use hc_proto::terminal::Terminal;
use serde_json::Value;
use std::collections::{BTreeMap, BTreeSet};

fn text(value: &Value, key: &str) -> String {
    value[key].as_str().unwrap_or_default().to_owned()
}

fn identity(kind: &str, id: &str) -> String {
    format!("{kind}:{}:{id}", id.len())
}

fn qualify(terminal: &str, machine: Option<&str>) -> String {
    machine.map_or_else(|| terminal.to_owned(), |m| format!("{m}/{terminal}"))
}

fn tiled_node(
    value: &Value,
    machine: Option<&str>,
    allowed: &BTreeSet<&str>,
    used: &mut BTreeSet<String>,
    depth: usize,
) -> Option<Tree> {
    if depth > 64 || used.len() >= 4096 {
        return None;
    }
    if value["type"] == "block" {
        let terminal = value["content"]["terminal"].as_str()?;
        let terminal = qualify(terminal, machine);
        return (allowed.contains(terminal.as_str()) && used.insert(terminal.clone()))
            .then(|| Tree::pane(terminal));
    }
    // Unknown node kinds may still have children. Unsupported mod content is
    // ignored individually; it never discards a neighbouring terminal.
    let children = value["children"].as_array()?;
    let mut nodes = Vec::new();
    for child in children.iter().take(4096) {
        if let Some(tree) = app_node(child, machine, allowed, used, depth + 1) {
            let weight = child["size"]["weight"]
                .as_f64()
                .filter(|v| v.is_finite() && *v > 0.0)
                .unwrap_or(1.0);
            nodes.push((tree, weight));
        }
    }
    let vertical = value["type"] == "column";
    let scale = nodes.iter().map(|(_, w)| *w).fold(0.0_f64, f64::max);
    for (_, weight) in &mut nodes {
        *weight = (*weight / scale).max(f64::MIN_POSITIVE);
    }
    fn combine(mut nodes: Vec<(Tree, f64)>, vertical: bool) -> Option<Tree> {
        if nodes.len() == 1 {
            return nodes.pop().map(|(tree, _)| tree);
        }
        if nodes.is_empty() {
            return None;
        }
        let rest = nodes.split_off(nodes.len() / 2);
        let left = nodes.iter().map(|(_, w)| w).sum::<f64>();
        let right = rest.iter().map(|(_, w)| w).sum::<f64>();
        Some(Tree::split(
            combine(nodes, vertical)?,
            combine(rest, vertical)?,
            vertical,
            (left / (left + right)).clamp(0.001, 0.999) as f32,
        ))
    }
    let tree = combine(nodes, vertical)?;
    if tree.valid() {
        Some(tree)
    } else {
        Tree::balanced(&tree.terminals(), vertical)
    }
}

fn app_node(
    value: &Value,
    machine: Option<&str>,
    allowed: &BTreeSet<&str>,
    used: &mut BTreeSet<String>,
    depth: usize,
) -> Option<Tree> {
    if depth > 64 {
        return None;
    }
    let tiled = tiled_node(value, machine, allowed, used, depth);
    let mut roots = tiled.into_iter().collect::<Vec<_>>();
    // Desktop floating terminals retain their space/tab membership. The TUI
    // tiles them alongside the node; mod-only floats remain ignored.
    if let Some(floats) = value["floating"].as_array() {
        for float in floats.iter().take(4096) {
            if let Some(raw) = float["content"]["terminal"].as_str() {
                let terminal = qualify(raw, machine);
                if allowed.contains(terminal.as_str()) && used.insert(terminal.clone()) {
                    roots.push(Tree::pane(terminal));
                }
            }
        }
    }
    let mut roots = roots.into_iter();
    let mut root = roots.next()?;
    for next in roots {
        root = Tree::split(root, next, false, 0.5);
    }
    if root.valid() {
        Some(root)
    } else {
        Tree::balanced(&root.terminals(), false)
    }
}

/// Foreign document versions and malformed individual entries are tolerated.
/// Only terminal membership matters: never execute mod data or rewrite it.
pub fn read(terms: &[&Terminal], document: &Value, machine: Option<&str>) -> Vec<Space> {
    let allowed = terms.iter().map(|t| t.address.as_str()).collect();
    let mut used = BTreeSet::new();
    let mut ids = BTreeSet::new();
    let mut spaces = Vec::new();
    if let Some(entries) = document["spaces"].as_array() {
        for space in entries.iter().take(1024) {
            let id = text(space, "id");
            if id.is_empty() || !ids.insert(identity("space", &id)) {
                continue;
            }
            let Some(tabs) = space["tabs"].as_array() else {
                continue;
            };
            let mut mapped = Vec::new();
            for tab in tabs.iter().take(4096) {
                let tab_id = text(tab, "id");
                let tab_id = format!("{}/{}", identity("space", &id), identity("tab", &tab_id));
                if !ids.insert(tab_id.clone()) {
                    continue;
                }
                if let Some(root) = app_node(&tab["root"], machine, &allowed, &mut used, 0) {
                    let name = text(tab, "name");
                    mapped.push(Tab {
                        id: tab_id,
                        custom_name: !name.is_empty(),
                        name,
                        root,
                    });
                }
            }
            if !mapped.is_empty() {
                let name = text(space, "name");
                let folder = text(space, "folder");
                let label = if name.is_empty() {
                    basename(&folder).to_owned()
                } else {
                    name.clone()
                };
                spaces.push(Space {
                    from_app: true,
                    id: identity("space", &id),
                    name: label,
                    custom_name: !name.is_empty(),
                    folder,
                    tabs: mapped,
                });
            }
        }
    }
    // Creation order is the reference's fallback, with the numeric terminal
    // id resolving equal timestamps. Keys retain raw (not display-cleaned) text.
    let mut rest = terms
        .iter()
        .copied()
        .filter(|t| !used.contains(&t.address))
        .collect::<Vec<_>>();
    rest.sort_by_key(|t| {
        (
            t.created_ms,
            crate::project::term_num(&t.address),
            t.address.as_str(),
        )
    });
    let mut positions = spaces
        .iter()
        .enumerate()
        .map(|(i, s)| (s.id.clone(), i))
        .collect::<BTreeMap<String, usize>>();
    let app_tabs = spaces
        .iter()
        .flat_map(|s| s.tabs.iter().map(|t| t.id.clone()))
        .collect::<BTreeSet<_>>();
    let mut pending = BTreeMap::<(usize, String), Vec<String>>::new();
    for terminal in rest {
        let (key, label) = crate::project::space_of(terminal);
        let at = *positions.entry(key.clone()).or_insert_with(|| {
            spaces.push(Space {
                from_app: false,
                id: key.clone(),
                name: label,
                custom_name: terminal.labels.contains_key("herdr.space"),
                folder: terminal.cwd.clone(),
                tabs: Vec::new(),
            });
            spaces.len() - 1
        });
        let tab_id = format!("{key}/{}", crate::project::tab_key_of(terminal));
        let tabs = &mut spaces[at].tabs;
        if let Some(tab) = tabs.iter_mut().find(|t| t.id == tab_id) {
            pending
                .entry((at, tab.id.clone()))
                .or_default()
                .push(terminal.address.clone());
        } else {
            let name = terminal
                .labels
                .get("herdr.tab")
                .cloned()
                .unwrap_or_default();
            tabs.push(Tab {
                id: tab_id,
                custom_name: !name.is_empty(),
                name,
                root: Tree::pane(&terminal.address),
            });
        }
    }
    for ((at, id), added) in pending {
        let tab = spaces[at]
            .tabs
            .iter_mut()
            .find(|t| t.id == id)
            .expect("tab was inserted");
        tab.root = if app_tabs.contains(&id) {
            Tree::split(
                tab.root.clone(),
                Tree::balanced(&added, false).expect("nonempty additions"),
                false,
                0.5,
            )
        } else {
            Tree::balanced(
                &tab.root
                    .terminals()
                    .into_iter()
                    .chain(added)
                    .collect::<Vec<_>>(),
                false,
            )
            .expect("nonempty membership")
        };
        if !tab.root.valid() {
            tab.root = Tree::balanced(&tab.root.terminals(), false).expect("nonempty membership");
        }
    }
    spaces
}

fn basename(path: &str) -> &str {
    path.trim_end_matches('/')
        .rsplit('/')
        .next()
        .filter(|s| !s.is_empty())
        .unwrap_or(path)
}
