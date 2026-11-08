//! Rebase user intentions on the latest document after a CAS conflict.
// Derived from herdr src/layout.rs and src/client/shell/actions.rs (Apache-2.0).
use crate::layout::NavDirection;
use crate::structure::{Document, Tree};
use ratatui::layout::{Direction, Rect};

#[derive(Debug, Clone)]
pub struct Edit {
    pub tab: String,
    pub space: String,
    /// Current published membership, used only when no override exists.
    pub fallback: Tree,
    pub incarnations: std::collections::BTreeMap<String, u64>,
    pub change: Change,
}

#[derive(Debug, Clone)]
pub enum Change {
    Focus(String),
    Split {
        target: String,
        terminal: String,
        vertical: bool,
        /// `keys.command` type="pane" splits then zooms, like the reference.
        zoomed: bool,
    },
    Swap {
        first: String,
        second: String,
    },
    Resize {
        terminal: String,
        direction: NavDirection,
        area: Rect,
        delta: f32,
    },
    Ratio {
        expected: Tree,
        path: Vec<bool>,
        ratio: f32,
    },
    Zoom(String),
    Name(String),
    NewTab {
        terminal: String,
        name: String,
    },
    MoveTab {
        tab_id: String,
        direction: isize,
        fallback: Vec<String>,
    },
}

impl Edit {
    pub fn apply(&self, document: &mut Document) -> Result<(), String> {
        let (tab, space) = super::rebase::canonical(document, &self.tab, &self.space);
        let mut edit = self.clone();
        edit.tab = tab;
        edit.space = space;
        if let Change::MoveTab {
            tab_id, fallback, ..
        } = &mut edit.change
        {
            let map = |id: &str| {
                let global = tab_key_from_space(&self.space, id);
                let (mapped, _) = super::rebase::canonical(document, &global, &self.space);
                local_key(&mapped).unwrap_or(id).to_owned()
            };
            *tab_id = map(tab_id);
            *fallback = fallback.iter().map(|id| map(id)).collect();
        }
        edit.apply_current(document)
    }

    fn apply_current(&self, document: &mut Document) -> Result<(), String> {
        if let Change::MoveTab {
            tab_id,
            direction,
            fallback,
        } = &self.change
        {
            let order = document
                .order
                .entry(self.space.clone())
                .or_insert_with(|| fallback.clone());
            order.retain(|id| {
                fallback.contains(id)
                    || document
                        .tabs
                        .get(&tab_key_from_space(&self.space, id))
                        .is_some_and(|t| t.root.is_some())
            });
            for id in fallback {
                if !order.contains(id) {
                    order.push(id.clone());
                }
            }
            let at = order
                .iter()
                .position(|id| id == tab_id)
                .ok_or("Tab no longer exists")?;
            if order.len() > 1 {
                let other = (at as isize + direction).rem_euclid(order.len() as isize) as usize;
                let moved = order.remove(at);
                order.insert(other, moved);
            }
            return Ok(());
        }
        let root = super::rebase::merge(
            self,
            document,
            document.tabs.get(&self.tab).and_then(|t| t.root.as_ref()),
        )?;
        let tab = document.tabs.entry(self.tab.clone()).or_default();
        let (mut layout, mut ids) = root.layout(tab.focused.as_deref());
        let find = |terminal: &str| {
            ids.iter()
                .find(|(_, name)| name.as_str() == terminal)
                .map(|(id, _)| *id)
        };
        let mut focus = None;
        match &self.change {
            Change::Focus(terminal) => {
                let id = find(terminal).ok_or("Pane no longer exists")?;
                layout.focus_pane(id);
                focus = Some(terminal.clone());
                if tab.zoomed.is_some() {
                    tab.zoomed = Some(terminal.clone());
                }
            }
            Change::Split {
                target,
                terminal,
                vertical,
                zoomed,
            } => {
                if find(terminal).is_none() {
                    let target = find(target).ok_or("Split target no longer exists")?;
                    let id = layout
                        .split_pane(
                            target,
                            if *vertical {
                                Direction::Vertical
                            } else {
                                Direction::Horizontal
                            },
                            0.5,
                        )
                        .ok_or("Split target no longer exists")?;
                    ids.insert(id, terminal.clone());
                    layout.focus_pane(id);
                }
                tab.zoomed = zoomed.then(|| terminal.clone());
                focus = Some(terminal.clone());
            }
            Change::Swap { first, second } => {
                let first = find(first).ok_or("Pane no longer exists")?;
                let second = find(second).ok_or("Pane no longer exists")?;
                layout.swap_panes(first, second);
            }
            Change::Resize {
                terminal,
                direction,
                area,
                delta,
            } => {
                let id = find(terminal).ok_or("Pane no longer exists")?;
                if !delta.is_finite() {
                    return Err("Invalid split ratio".into());
                }
                layout.resize_pane(id, *direction, *delta, *area);
            }
            Change::Ratio {
                expected,
                path,
                ratio,
            } => {
                if !super::rebase::same_topology(&root, expected) {
                    return Err("Split topology changed; start the drag again".into());
                }
                if !layout.set_ratio_at(path, *ratio) {
                    return Err("Split no longer exists".into());
                }
            }
            Change::Zoom(terminal) => {
                find(terminal).ok_or("Pane no longer exists")?;
                tab.zoomed = if tab.zoomed.as_ref() == Some(terminal) {
                    None
                } else {
                    Some(terminal.clone())
                };
                focus = Some(terminal.clone());
            }
            Change::Name(name) => tab.name = Some(name.clone()),
            Change::NewTab { terminal, name } => {
                let (single, mapping) = Tree::pane(terminal).layout(Some(terminal));
                layout = single;
                ids = mapping;
                tab.name = Some(name.clone());
                focus = Some(terminal.clone());
            }
            Change::MoveTab { .. } => unreachable!(),
        }
        tab.root = Tree::from_layout(&layout, &ids);
        if tab.root.as_ref().is_none_or(|r| !r.valid()) {
            return Err("Invalid resulting layout".into());
        }
        if let Some(focus) = focus {
            if tab.focused.as_ref() != Some(&focus) {
                tab.previous = tab.focused.clone();
            }
            tab.focused = Some(focus.clone());
            document.focus = Some(focus);
            document.active.insert(self.space.clone(), self.tab.clone());
        }
        Ok(())
    }
}

/// Copy old fallback identities after adding adoption labels. Never overwrite
/// a destination already edited by another client, nor remove opaque state.
pub fn migrate(document: &mut Document, aliases: &[(String, String)]) {
    for (old, new) in aliases {
        if let Some(tab) = document.tabs.get(old).cloned() {
            document.tabs.entry(new.clone()).or_insert(tab);
        }
        if let Some(order) = document.order.get(old).cloned()
            && let Some(local) = local_key(old)
        {
            let prefix = &old[..old.len() - local.len()];
            let mapped = order
                .into_iter()
                .map(|id| {
                    let global = format!("{prefix}{id}");
                    aliases
                        .iter()
                        .find(|(from, _)| from == &global)
                        .and_then(|(_, to)| local_key(to))
                        .map(str::to_owned)
                        .unwrap_or(id)
                })
                .collect::<Vec<_>>();
            let target = document.order.entry(new.clone()).or_default();
            for id in mapped {
                if !target.contains(&id) {
                    target.push(id);
                }
            }
        }
        if let Some(active) = document.active.get(old).cloned() {
            let mapped = aliases
                .iter()
                .find(|(from, _)| from == &active)
                .map(|(_, to)| to.clone())
                .unwrap_or(active);
            document.active.entry(new.clone()).or_insert(mapped);
        }
    }
}

pub fn hide(document: &mut Document, terminal: &str, created_ms: u64) {
    document.closed.insert(terminal.to_owned(), created_ms);
    let mut replacement = None;
    for tab in document.tabs.values_mut() {
        if let Some(root) = &tab.root {
            tab.root = root.retain(&|id| id != terminal);
        }
        let contains_focus = tab.focused.as_deref() == Some(terminal);
        if tab.focused.as_deref() == Some(terminal) {
            tab.focused = tab
                .previous
                .clone()
                .filter(|id| {
                    tab.root
                        .as_ref()
                        .is_some_and(|r| r.terminals().contains(id))
                })
                .or_else(|| {
                    tab.root
                        .as_ref()
                        .and_then(|r| r.terminals().first().cloned())
                });
        }
        if contains_focus {
            replacement = tab.focused.clone();
        }
        if tab.zoomed.as_deref() == Some(terminal) {
            tab.zoomed = None;
        }
    }
    if document.focus.as_deref() == Some(terminal) {
        document.focus = replacement;
    }
}

pub fn tab_key(machine: Option<&str>, tab: &str) -> String {
    crate::project::layout_key(machine, tab)
}

pub(super) fn local_key(key: &str) -> Option<&str> {
    let (length, tail) = key.split_once(':')?;
    tail.get(length.parse::<usize>().ok()?..)?.strip_prefix('/')
}

fn tab_key_from_space(space: &str, local_tab: &str) -> String {
    let local = local_key(space).unwrap_or(space);
    format!("{}{local_tab}", &space[..space.len() - local.len()])
}
