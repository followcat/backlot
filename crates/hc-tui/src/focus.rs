#![allow(dead_code)] // Group 2/3 variants & methods wired incrementally across Part 1 commits.
//! Unified client focus model (`FocusState` and `WorkspaceRegion`).
//!
//! Replaces scattered surface focus booleans with one authoritative focus
//! stack: zero or more [`ModalLayer`] entries sitting over one
//! [`WorkspaceRegion`] (`Terminal` by default).

use std::path::Path;

use crate::config::KeyCombo;
use crate::studio::StudioKey;

/// Target recipient when composing or telling from inside the Lot.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TellTarget {
    Role(String),
    Hollywood,
}

impl Default for TellTarget {
    fn default() -> Self {
        Self::Role("producer".to_owned())
    }
}

impl TellTarget {
    pub fn as_str(&self) -> &str {
        match self {
            Self::Role(role) => role.as_str(),
            Self::Hollywood => "hollywood",
        }
    }

    pub fn is_hollywood(&self) -> bool {
        matches!(self, Self::Hollywood)
    }

    /// Cycle empty compose targets: `producer -> cursor role -> hollywood`.
    pub fn cycle_next(&self, cursor_role: Option<&str>) -> Self {
        let distinct_role = cursor_role.filter(|r| *r != "producer" && *r != "hollywood");
        match self {
            Self::Role(r) if r == "producer" => match distinct_role {
                Some(role) => Self::Role(role.to_owned()),
                None => Self::Hollywood,
            },
            Self::Role(_) => Self::Hollywood,
            Self::Hollywood => Self::Role("producer".to_owned()),
        }
    }
}

/// Nested navigation depth inside the Lot Studio Dock.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LotLevel {
    /// Level 1: studio block headers (+ new studio, setup hint).
    Studios,
    /// Level 2: inside an unfolded studio block.
    InsideStudio(StudioKey),
    /// Level 3a: focused on a specific inline action chip on a row.
    RowActions { studio: StudioKey, chip: usize },
    /// Level 3b: inline compose editor active inside `StudioKey`.
    Compose {
        studio: StudioKey,
        target: TellTarget,
    },
}

/// Active zone inside the Call Sheet dock, in screen and `Tab` order
/// (`WAITING -> HOLLYWOOD -> STUDIOS`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CallSheetZone {
    Waiting,
    Hollywood,
    Studios,
}

/// Non-modal workspace regions that share one global switching contract.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum WorkspaceRegion {
    Terminal,
    /// Left sidebar top section: machines, live studios, archived studios.
    Tree,
    /// Left sidebar bottom section: current studio's cast roles and shells.
    Cast,
    /// Right-hand Lot / Studio Dock (expanded column, narrow overlay, or dead-pane view).
    Lot(LotLevel),
    /// Top floating Hollywood Call Sheet.
    CallSheet(CallSheetZone),
}

/// Modal layers pushed above the active [`WorkspaceRegion`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ModalLayer {
    Popup,
    Peek,
    ContextMenu,
    Dialog,
    StudioDialog,
    HwSwitch,
    NewStudio,
    TaskSheet,
    Reader,
    UpdateCard,
    HelpLauncher,
    Navigator,
    Settings,
}

/// Authoritative focus stack for the entire client.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FocusState {
    pub workspace: WorkspaceRegion,
    pub modals: Vec<ModalLayer>,
}

impl Default for FocusState {
    fn default() -> Self {
        Self {
            workspace: WorkspaceRegion::Terminal,
            modals: Vec::new(),
        }
    }
}

impl FocusState {
    /// Whether plain `Ctrl+G` keyboard lock is allowed right now.
    pub fn allows_keyboard_lock(&self) -> bool {
        self.modals.is_empty() && self.workspace == WorkspaceRegion::Terminal
    }

    /// Reconcile stacked modals against live App modal flags while
    /// preserving stack order (e.g. `TaskSheet -> Reader`).
    pub fn sync_modals(&mut self, active: &[ModalLayer]) {
        self.modals.retain(|m| active.contains(m));
        for layer in active {
            if !self.modals.contains(layer) {
                self.modals.push(*layer);
            }
        }
    }

    /// Compact modebar breadcrumb while focus is outside `Terminal`.
    pub fn breadcrumb(&self, studio: Option<&str>, detail: Option<&str>) -> Option<String> {
        if !self.modals.is_empty() {
            return None;
        }
        let body = match &self.workspace {
            WorkspaceRegion::Terminal => return None,
            WorkspaceRegion::Tree => "tree".to_owned(),
            WorkspaceRegion::Cast => match studio {
                Some(s) => format!("cast › {s}"),
                None => "cast".to_owned(),
            },
            WorkspaceRegion::Lot(level) => match level {
                LotLevel::Studios => "lot › studios".to_owned(),
                LotLevel::InsideStudio(k) => match detail {
                    Some(d) if !d.is_empty() => format!("lot › {} › {d}", k.id),
                    _ => format!("lot › {}", k.id),
                },
                LotLevel::RowActions { studio: k, .. } => match detail {
                    Some(d) if !d.is_empty() => format!("lot › {} › {d}", k.id),
                    _ => format!("lot › {} › action", k.id),
                },
                LotLevel::Compose { studio: k, target } => {
                    format!("lot › {} › tell {}", k.id, target.as_str())
                }
            },
            WorkspaceRegion::CallSheet(zone) => {
                let z = match zone {
                    CallSheetZone::Waiting => "waiting",
                    CallSheetZone::Hollywood => "hollywood",
                    CallSheetZone::Studios => "studios",
                };
                format!("call sheet › {z}")
            }
        };
        Some(format!("FOCUS: {body}"))
    }
}

/// Cursor target inside the left sidebar's Cast section.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CastTarget {
    Role { studio: StudioKey, role: String },
    ClosedRoles { studio: StudioKey },
    Shell { studio: StudioKey, terminal: String },
    OthersToggle,
    OtherAgent(String),
}

/// Configurable focus key binding (`"prefix+t"`, `"alt+t"`, or `""` unbound).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum FocusBinding {
    Unbound,
    Direct(KeyCombo),
    Chord(KeyCombo),
}

impl FocusBinding {
    pub(crate) fn parse(
        raw: Option<String>,
        default: &str,
        name: &str,
        path: &Path,
        notice: &mut Option<String>,
    ) -> Self {
        let parse_spec = |spec: &str| -> Option<FocusBinding> {
            let trimmed = spec.trim();
            if trimmed.is_empty() {
                return Some(FocusBinding::Unbound);
            }
            if let Some(rest) = trimmed.strip_prefix("prefix+") {
                crate::config::parse_key_combo(rest).map(FocusBinding::Chord)
            } else {
                let combo = crate::config::parse_key_combo(trimmed)?;
                (!crate::config::is_unmodified_printable(combo))
                    .then_some(FocusBinding::Direct(combo))
            }
        };
        let fallback = || parse_spec(default).expect("default focus binding parses");
        match raw {
            None => fallback(),
            Some(ref value) if value.trim().is_empty() => FocusBinding::Unbound,
            Some(value) => match parse_spec(&value) {
                Some(b) => b,
                None => {
                    *notice = Some(format!("{path:?} ignored: invalid keys.{name}"));
                    fallback()
                }
            },
        }
    }

    pub(crate) fn matches_direct(&self, key: &crate::config::TerminalKey) -> bool {
        matches!(self, Self::Direct(combo) if crate::config::terminal_key_matches_combo(key, *combo))
    }

    pub(crate) fn matches_chord(&self, key: &crate::config::TerminalKey) -> bool {
        matches!(self, Self::Chord(combo) if crate::config::terminal_key_matches_combo(key, *combo))
    }

    pub(crate) fn label(&self, prefixes: &[KeyCombo]) -> String {
        match self {
            Self::Unbound => "unset".to_owned(),
            Self::Direct(combo) => crate::config::format_key_combo(*combo),
            Self::Chord(combo) => format!(
                "{} {}",
                crate::config::format_prefix_combos(prefixes),
                crate::config::format_key_combo(*combo)
            ),
        }
    }
}
