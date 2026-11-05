// Derived from herdr src/client/shell.rs, src/api/schema/common.rs, and
// src/config/model.rs (Apache-2.0).

use ratatui::style::Color;

use super::palette::Palette;
use crate::config::StatusIndicatorStyle;

/// Agent lifecycle status as shown by the shell chrome.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum AgentStatus {
    Idle,
    Working,
    Blocked,
    Done,
    Unknown,
}

pub(crate) fn status_icon(status: AgentStatus, style: StatusIndicatorStyle) -> &'static str {
    match (style, status) {
        (
            StatusIndicatorStyle::Dots,
            AgentStatus::Working | AgentStatus::Blocked | AgentStatus::Done,
        ) => "●",
        (StatusIndicatorStyle::Dots, AgentStatus::Idle) => "○",
        (StatusIndicatorStyle::Dots, AgentStatus::Unknown) => "·",
        (StatusIndicatorStyle::Symbols, AgentStatus::Blocked) => "×",
        (StatusIndicatorStyle::Symbols, AgentStatus::Working) => "◐",
        (StatusIndicatorStyle::Symbols, AgentStatus::Done) => "✓",
        (StatusIndicatorStyle::Symbols, AgentStatus::Idle) => "○",
        (StatusIndicatorStyle::Symbols, AgentStatus::Unknown) => "·",
    }
}

pub(crate) fn status_priority(status: AgentStatus) -> u8 {
    match status {
        AgentStatus::Blocked => 4,
        AgentStatus::Done => 3,
        AgentStatus::Working => 2,
        AgentStatus::Idle => 1,
        AgentStatus::Unknown => 0,
    }
}

pub(crate) fn status_text(status: AgentStatus) -> &'static str {
    match status {
        AgentStatus::Working => "working",
        AgentStatus::Blocked => "blocked",
        AgentStatus::Done => "done",
        AgentStatus::Idle => "idle",
        AgentStatus::Unknown => "unknown",
    }
}

pub(crate) fn status_color(status: AgentStatus, palette: &Palette) -> Color {
    match status {
        AgentStatus::Working => palette.yellow,
        AgentStatus::Blocked => palette.red,
        AgentStatus::Done => palette.teal,
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
        assert!(status_priority(AgentStatus::Working) > status_priority(AgentStatus::Idle));
        assert!(status_priority(AgentStatus::Idle) > status_priority(AgentStatus::Unknown));
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
