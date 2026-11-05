// Derived from herdr src/layout.rs (Apache-2.0).
use crate::layout::{Node, PaneId, TileLayout};
use ratatui::layout::Direction;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};

/// Persistent terminal addresses, independent of the reference's process-local
/// pane allocator. Splits use the reference's horizontal/vertical semantics.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "lowercase", deny_unknown_fields)]
pub enum Tree {
    Pane {
        terminal: String,
    },
    Split {
        vertical: bool,
        ratio: f32,
        first: Box<Tree>,
        second: Box<Tree>,
    },
}

impl Tree {
    pub fn pane(terminal: impl Into<String>) -> Self {
        Self::Pane {
            terminal: terminal.into(),
        }
    }

    pub fn split(first: Self, second: Self, vertical: bool, ratio: f32) -> Self {
        Self::Split {
            vertical,
            ratio,
            first: Box::new(first),
            second: Box::new(second),
        }
    }

    pub fn terminals(&self) -> Vec<String> {
        match self {
            Self::Pane { terminal } => vec![terminal.clone()],
            Self::Split { first, second, .. } => first
                .terminals()
                .into_iter()
                .chain(second.terminals())
                .collect(),
        }
    }

    pub fn valid(&self) -> bool {
        fn visit(tree: &Tree, depth: usize, seen: &mut BTreeSet<String>) -> bool {
            if depth > 64 {
                return false;
            }
            match tree {
                Tree::Pane { terminal } => !terminal.is_empty() && seen.insert(terminal.clone()),
                Tree::Split {
                    ratio,
                    first,
                    second,
                    ..
                } => {
                    ratio.is_finite()
                        && *ratio > 0.0
                        && *ratio < 1.0
                        && visit(first, depth + 1, seen)
                        && visit(second, depth + 1, seen)
                }
            }
        }
        visit(self, 0, &mut BTreeSet::new())
    }

    /// Remove stale membership and collapse an empty side without changing
    /// the ratios of surviving ancestors.
    pub fn retain(&self, keep: &impl Fn(&str) -> bool) -> Option<Self> {
        match self {
            Self::Pane { terminal } => keep(terminal).then(|| self.clone()),
            Self::Split {
                vertical,
                ratio,
                first,
                second,
            } => match (first.retain(keep), second.retain(keep)) {
                (Some(a), Some(b)) => Some(Self::split(a, b, *vertical, *ratio)),
                (a, b) => a.or(b),
            },
        }
    }

    pub fn balanced(terminals: &[String], vertical: bool) -> Option<Self> {
        match terminals {
            [] => None,
            [terminal] => Some(Self::pane(terminal)),
            many => {
                let middle = many.len() / 2;
                Some(Self::split(
                    Self::balanced(&many[..middle], vertical)?,
                    Self::balanced(&many[middle..], vertical)?,
                    vertical,
                    middle as f32 / many.len() as f32,
                ))
            }
        }
    }

    pub fn layout(&self, focused: Option<&str>) -> (TileLayout, BTreeMap<PaneId, String>) {
        fn node(tree: &Tree, ids: &mut BTreeMap<PaneId, String>) -> Node {
            match tree {
                Tree::Pane { terminal } => {
                    let id = PaneId::alloc();
                    ids.insert(id, terminal.clone());
                    Node::Pane(id)
                }
                Tree::Split {
                    vertical,
                    ratio,
                    first,
                    second,
                } => Node::Split {
                    direction: if *vertical {
                        Direction::Vertical
                    } else {
                        Direction::Horizontal
                    },
                    ratio: *ratio,
                    first: Box::new(node(first, ids)),
                    second: Box::new(node(second, ids)),
                },
            }
        }
        let mut ids = BTreeMap::new();
        let root = node(self, &mut ids);
        let focus = ids
            .iter()
            .find(|(_, id)| Some(id.as_str()) == focused)
            .or_else(|| ids.first_key_value())
            .map(|(id, _)| *id)
            .expect("a tree has a leaf");
        (TileLayout::from_saved(root, focus), ids)
    }

    pub fn from_layout(layout: &TileLayout, ids: &BTreeMap<PaneId, String>) -> Option<Self> {
        fn tree(node: &Node, ids: &BTreeMap<PaneId, String>) -> Option<Tree> {
            Some(match node {
                Node::Pane(id) => Tree::pane(ids.get(id)?),
                Node::Split {
                    direction,
                    ratio,
                    first,
                    second,
                } => Tree::split(
                    tree(first, ids)?,
                    tree(second, ids)?,
                    *direction == Direction::Vertical,
                    *ratio,
                ),
            })
        }
        tree(layout.root(), ids)
    }
}
