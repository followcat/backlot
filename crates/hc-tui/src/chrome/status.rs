// Derived from herdr src/client/shell.rs, src/api/schema/common.rs, and
// src/config/model.rs (Apache-2.0).

use ratatui::style::Color;

use super::palette::Palette;
use crate::config::StatusIndicatorStyle;

/// Agent lifecycle status as shown by the shell chrome.
///
/// `Waiting` is the one status no core word maps to: it is a
/// client-side overlay on a shown Idle while probe evidence says the
/// agent's own turn is quiet but work it started still runs
/// (docs/sidebar.md).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum AgentStatus {
    Idle,
    Working,
    Blocked,
    Done,
    Unknown,
    Waiting,
}

pub(crate) fn status_icon(status: AgentStatus, style: StatusIndicatorStyle) -> &'static str {
    match (style, status) {
        (
            StatusIndicatorStyle::Dots,
            AgentStatus::Working | AgentStatus::Blocked | AgentStatus::Done,
        ) => "●",
        (StatusIndicatorStyle::Dots, AgentStatus::Idle) => "○",
        (StatusIndicatorStyle::Dots, AgentStatus::Waiting) => "◌",
        (StatusIndicatorStyle::Dots, AgentStatus::Unknown) => "·",
        (StatusIndicatorStyle::Symbols, AgentStatus::Blocked) => "×",
        (StatusIndicatorStyle::Symbols, AgentStatus::Working) => "◐",
        (StatusIndicatorStyle::Symbols, AgentStatus::Done) => "✓",
        (StatusIndicatorStyle::Symbols, AgentStatus::Idle) => "○",
        (StatusIndicatorStyle::Symbols, AgentStatus::Waiting) => "◌",
        (StatusIndicatorStyle::Symbols, AgentStatus::Unknown) => "·",
    }
}

pub(crate) fn status_priority(status: AgentStatus) -> u8 {
    match status {
        AgentStatus::Blocked => 5,
        AgentStatus::Done => 4,
        AgentStatus::Working => 3,
        AgentStatus::Waiting => 2,
        AgentStatus::Idle => 1,
        AgentStatus::Unknown => 0,
    }
}

pub(crate) fn status_text(status: AgentStatus) -> &'static str {
    match status {
        AgentStatus::Working => "working",
        AgentStatus::Blocked => "blocked",
        AgentStatus::Done => "done",
        AgentStatus::Waiting => "waiting",
        AgentStatus::Idle => "idle",
        AgentStatus::Unknown => "unknown",
    }
}

pub(crate) fn status_color(status: AgentStatus, palette: &Palette) -> Color {
    match status {
        AgentStatus::Working => palette.yellow,
        AgentStatus::Blocked => palette.red,
        AgentStatus::Done => palette.teal,
        AgentStatus::Waiting => palette.peach,
        AgentStatus::Idle => palette.green,
        AgentStatus::Unknown => palette.overlay0,
    }
}

pub(crate) fn panel_contrast_fg(palette: &Palette) -> Color {
    match palette.panel_bg {
        Color::Reset => palette.surface_dim,
        color => color,
    }
}

/// Map an hc-core status word to herdr's presentation status. Core's
/// `done`/`ready`/`finished` vocabulary has no direct seedable herdr
/// `AgentState` counterpart; the documented mapping lands in docs/parity.md.
pub(crate) fn agent_status_from_word(word: &str) -> AgentStatus {
    match word {
        "working" => AgentStatus::Working,
        "blocked" => AgentStatus::Blocked,
        "done" | "ready" | "finished" => AgentStatus::Done,
        "idle" => AgentStatus::Idle,
        _ => AgentStatus::Unknown,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn priority_orders_blocked_first() {
        assert!(status_priority(AgentStatus::Blocked) > status_priority(AgentStatus::Done));
        assert!(status_priority(AgentStatus::Done) > status_priority(AgentStatus::Working));
        assert!(status_priority(AgentStatus::Working) > status_priority(AgentStatus::Waiting));
        assert!(status_priority(AgentStatus::Waiting) > status_priority(AgentStatus::Idle));
        assert!(status_priority(AgentStatus::Idle) > status_priority(AgentStatus::Unknown));
    }

    #[test]
    fn waiting_is_single_width_and_distinct_in_both_styles() {
        let others = [
            AgentStatus::Idle,
            AgentStatus::Working,
            AgentStatus::Blocked,
            AgentStatus::Done,
            AgentStatus::Unknown,
        ];
        for style in [StatusIndicatorStyle::Dots, StatusIndicatorStyle::Symbols] {
            let icon = status_icon(AgentStatus::Waiting, style);
            assert_eq!(unicode_width::UnicodeWidthStr::width(icon), 1, "{icon}");
            for other in others {
                assert_ne!(
                    icon,
                    status_icon(other, style),
                    "{style:?} Waiting icon collides with {other:?}"
                );
            }
        }
        assert_eq!(status_text(AgentStatus::Waiting), "waiting");
    }

    #[test]
    fn waiting_has_its_own_color() {
        let palette = Palette::catppuccin();
        let color = status_color(AgentStatus::Waiting, &palette);
        assert_ne!(color, palette.accent);
        for other in [
            AgentStatus::Idle,
            AgentStatus::Working,
            AgentStatus::Blocked,
            AgentStatus::Done,
            AgentStatus::Unknown,
        ] {
            assert_ne!(color, status_color(other, &palette), "{other:?}");
        }
    }

    /// `waiting` is a client overlay, never a core word: no status
    /// record maps to it.
    #[test]
    fn no_core_word_maps_to_waiting() {
        for word in ["idle", "working", "done", "waiting", "blocked", ""] {
            assert_ne!(agent_status_from_word(word), AgentStatus::Waiting);
        }
    }

    #[test]
    fn glyphs_match_reference() {
        assert_eq!(
            status_icon(AgentStatus::Working, StatusIndicatorStyle::Dots),
            "●"
        );
        assert_eq!(
            status_icon(AgentStatus::Idle, StatusIndicatorStyle::Dots),
            "○"
        );
        assert_eq!(
            status_icon(AgentStatus::Unknown, StatusIndicatorStyle::Dots),
            "·"
        );
        assert_eq!(
            status_icon(AgentStatus::Blocked, StatusIndicatorStyle::Symbols),
            "×"
        );
        assert_eq!(
            status_icon(AgentStatus::Working, StatusIndicatorStyle::Symbols),
            "◐"
        );
        assert_eq!(
            status_icon(AgentStatus::Done, StatusIndicatorStyle::Symbols),
            "✓"
        );
    }
}
