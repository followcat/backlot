//! Derived *Babylon* deco tokens (`gilt`, `stage_well`, `bulb_warm`) and
//! contrast guards computed from resolved `Palette` / `Elevation` bases.

use ratatui::style::{Color, Modifier, Style};

use super::elevation::{Elevation, contrast_ratio, mix, move_lstar, to_color};

/// Ornamental frame/sill tokens derived alongside `Elevation`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct DecoTokens {
    pub(super) gilt: Color,
    pub(super) stage_well: Color,
    pub(super) bulb_warm: Color,
}

pub(super) const TERMINAL_DECO: DecoTokens = DecoTokens {
    gilt: Color::Yellow,
    stage_well: Color::Reset,
    bulb_warm: Color::Yellow,
};

pub(super) fn derive_deco(
    is_dark: bool,
    panel_bg: (u8, u8, u8),
    surface: (u8, u8, u8),
    surface0: (u8, u8, u8),
    text: (u8, u8, u8),
    accent: (u8, u8, u8),
    yellow: (u8, u8, u8),
) -> DecoTokens {
    let stage_well_rgb = if is_dark {
        panel_bg
    } else {
        move_lstar(surface, surface0, text, 4.0, false)
    };
    let gilt_t = if is_dark { 0.40 } else { 0.50 };
    let gilt_accent = guard_fg(mix(surface, accent, gilt_t), surface, accent, 2.65);
    let gilt_guarded = guard_fg(gilt_accent, surface, text, 2.65);
    let gilt_rgb = clamp_max_contrast(gilt_guarded, surface, 3.20);
    let bulb_pole = if is_dark { (255, 255, 255) } else { (0, 0, 0) };
    let bulb_yellow = guard_fg(
        mix(stage_well_rgb, yellow, 0.62),
        stage_well_rgb,
        yellow,
        4.5,
    );
    let bulb_warm_rgb = guard_fg(bulb_yellow, stage_well_rgb, bulb_pole, 4.5);

    DecoTokens {
        gilt: to_color(gilt_rgb),
        stage_well: to_color(stage_well_rgb),
        bulb_warm: to_color(bulb_warm_rgb),
    }
}

impl Elevation {
    /// Foreground gilt style on `bg`, adding `DIM` on 16-color terminals.
    pub(crate) fn gilt_style(&self, bg: Color) -> Style {
        let style = Style::default().fg(self.gilt).bg(bg);
        if self.is_terminal_16 {
            style.add_modifier(Modifier::DIM)
        } else {
            style
        }
    }
}

pub(super) fn guard_fg(
    candidate: (u8, u8, u8),
    bg: (u8, u8, u8),
    target: (u8, u8, u8),
    min_cr: f64,
) -> (u8, u8, u8) {
    if contrast_ratio(candidate, bg) >= min_cr {
        return candidate;
    }
    let mut lo = 0.0_f64;
    let mut hi = 1.0_f64;
    for _ in 0..24 {
        let mid = 0.5 * (lo + hi);
        if contrast_ratio(mix(candidate, target, mid), bg) >= min_cr {
            hi = mid;
        } else {
            lo = mid;
        }
    }
    mix(candidate, target, hi)
}

pub(super) fn clamp_max_contrast(
    candidate: (u8, u8, u8),
    bg: (u8, u8, u8),
    max_cr: f64,
) -> (u8, u8, u8) {
    if contrast_ratio(candidate, bg) <= max_cr {
        return candidate;
    }
    let mut lo = 0.0_f64;
    let mut hi = 1.0_f64;
    for _ in 0..24 {
        let mid = 0.5 * (lo + hi);
        if contrast_ratio(mix(candidate, bg, mid), bg) > max_cr {
            lo = mid;
        } else {
            hi = mid;
        }
    }
    mix(candidate, bg, hi)
}
