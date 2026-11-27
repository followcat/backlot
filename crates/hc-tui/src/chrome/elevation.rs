//! Derived elevation & depth colors for any `Palette` — zero hard-coded
//! RGB colors and zero theme-name switches. Every level is derived from
//! `Palette` surface and text tokens using sRGB luminance `Y`, perceptual
//! CIE lightness `L*`, and WCAG contrast ratio.

use ratatui::buffer::Cell;
use ratatui::style::{Color, Modifier};

use super::elevation_deco::{TERMINAL_DECO, clamp_max_contrast, derive_deco, guard_fg};
use super::palette::Palette;

/// Derived surface & depth levels for rendering chrome, overlays, and dim/shadow.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Elevation {
    pub base: Color,
    pub surface: Color,
    pub overlay: Color,
    pub raised_row: Color,
    pub raised_field: Color,
    pub raised_on_overlay: Color,
    pub dim_bg: Option<Color>,
    pub shadow: Option<Color>,
    pub border_muted: Color,
    pub gilt: Color,
    pub stage_well: Color,
    pub bulb_warm: Color,
    pub muted_surface: Color,
    pub muted_overlay: Color,
    pub muted_raised: Color,
    pub is_dark: bool,
    pub is_terminal_16: bool,
}

impl Elevation {
    pub(crate) fn from_palette(palette: &Palette) -> Self {
        let (Some(panel_bg), Some(text)) = (rgb_of(palette.panel_bg), rgb_of(palette.text)) else {
            return Self {
                base: Color::Reset,
                surface: Color::Reset,
                overlay: Color::Reset,
                raised_row: palette.active_row_bg,
                raised_field: Color::Reset,
                raised_on_overlay: Color::Reset,
                dim_bg: None,
                shadow: None,
                border_muted: palette.overlay0,
                gilt: TERMINAL_DECO.gilt,
                stage_well: TERMINAL_DECO.stage_well,
                bulb_warm: TERMINAL_DECO.bulb_warm,
                muted_surface: palette.overlay0,
                muted_overlay: palette.overlay0,
                muted_raised: palette.overlay0,
                is_dark: true,
                is_terminal_16: true,
            };
        };

        let surface0 = rgb_of(palette.surface0).unwrap_or(panel_bg);
        let surface1 = rgb_of(palette.surface1);
        let selection_bg = rgb_of(palette.selection_bg);
        let overlay0 = rgb_of(palette.overlay0).unwrap_or(text);
        let accent = rgb_of(palette.accent).unwrap_or(text);
        let yellow = rgb_of(palette.yellow).unwrap_or(text);
        let is_dark = luminance(panel_bg) < luminance(text);

        let (
            surface_rgb,
            overlay_rgb,
            raised_row_rgb,
            raised_field_rgb,
            raised_overlay_rgb,
            dim_bg_rgb,
            shadow_rgb,
            border_muted_rgb,
        ) = if is_dark {
            let darkest = match rgb_of(palette.sidebar_bg) {
                Some(sb) if luminance(sb) < luminance(panel_bg) => sb,
                _ => panel_bg,
            };
            let surface = move_lstar(panel_bg, surface0, text, 6.5, true);
            let overlay = move_lstar(panel_bg, surface0, text, 9.5, true);
            let raised = pick_raised_dark(overlay, surface, selection_bg, surface0, text);
            let dim_bg = scale_rgb(darkest, 0.70);
            let shadow = scale_rgb(darkest, 0.45);
            let border_muted = match surface1 {
                Some(s1) if (1.4..=3.2).contains(&contrast_ratio(s1, surface)) => s1,
                _ => {
                    let candidate = move_lstar(surface, overlay0, text, 12.0, true);
                    guard_fg(candidate, surface, text, 1.4)
                }
            };
            (
                surface,
                overlay,
                raised,
                raised,
                raised,
                dim_bg,
                shadow,
                border_muted,
            )
        } else {
            let surface = move_lstar(panel_bg, surface0, text, 5.5, false);
            let overlay = panel_bg;
            let raised_row = pick_raised_light(surface, selection_bg, surface0, text);
            let raised_field = panel_bg;
            let raised_on_overlay = surface;
            let dim_bg = move_lstar(panel_bg, surface0, text, 9.5, false);
            let shadow = move_lstar(panel_bg, surface0, text, 17.0, false);
            let guarded = guard_fg(overlay0, surface, text, 1.8);
            let border_muted = clamp_max_contrast(guarded, surface, 3.2);
            (
                surface,
                overlay,
                raised_row,
                raised_field,
                raised_on_overlay,
                dim_bg,
                shadow,
                border_muted,
            )
        };

        let deco = derive_deco(
            is_dark,
            panel_bg,
            surface_rgb,
            surface0,
            text,
            accent,
            yellow,
        );

        let muted_surface = guard_fg(overlay0, surface_rgb, text, 3.0);
        let muted_overlay = guard_fg(overlay0, overlay_rgb, text, 3.0);
        let muted_raised = guard_fg(overlay0, raised_field_rgb, text, 3.0);

        Self {
            base: Color::Reset,
            surface: to_color(surface_rgb),
            overlay: to_color(overlay_rgb),
            raised_row: to_color(raised_row_rgb),
            raised_field: to_color(raised_field_rgb),
            raised_on_overlay: to_color(raised_overlay_rgb),
            dim_bg: Some(to_color(dim_bg_rgb)),
            shadow: Some(to_color(shadow_rgb)),
            border_muted: to_color(border_muted_rgb),
            gilt: deco.gilt,
            stage_well: deco.stage_well,
            bulb_warm: deco.bulb_warm,
            muted_surface: to_color(muted_surface),
            muted_overlay: to_color(muted_overlay),
            muted_raised: to_color(muted_raised),
            is_dark,
            is_terminal_16: false,
        }
    }

    /// Attenuate one backdrop cell outside a modal popup.
    pub(crate) fn dim_cell(&self, cell: &mut Cell, palette: &Palette) {
        strip_backdrop_bits(cell);
        let Some(Color::Rgb(dr, dg, db)) = self.dim_bg else {
            cell.modifier.insert(Modifier::DIM);
            return;
        };
        let dim_rgb = (dr, dg, db);
        let bg_t = if self.is_dark { 0.55 } else { 0.50 };
        let mut fallback_dim = false;

        cell.bg = match cell.bg {
            Color::Reset => to_color(dim_rgb),
            Color::Rgb(r, g, b) => to_color(mix((r, g, b), dim_rgb, bg_t)),
            other => {
                fallback_dim = true;
                other
            }
        };

        let text_rgb = rgb_of(palette.text);
        cell.fg = match cell.fg {
            Color::Reset => match text_rgb {
                Some(fg) => to_color(mix(fg, dim_rgb, 0.55)),
                None => {
                    fallback_dim = true;
                    Color::Reset
                }
            },
            Color::Rgb(r, g, b) => to_color(mix((r, g, b), dim_rgb, 0.55)),
            other => {
                fallback_dim = true;
                other
            }
        };

        if fallback_dim {
            cell.modifier.insert(Modifier::DIM);
        }
    }

    /// Apply the 1-cell drop shadow onto one cell outside a popup or frame.
    /// Keeps the underlying symbol untouched and blends foreground 72% toward
    /// the shadow background (no-op on 16-color terminal palettes).
    pub(crate) fn shadow_cell(&self, cell: &mut Cell, palette: &Palette) {
        let Some(Color::Rgb(sr, sg, sb)) = self.shadow else {
            return;
        };
        strip_backdrop_bits(cell);
        let shadow_rgb = (sr, sg, sb);
        cell.bg = to_color(shadow_rgb);
        let text_rgb = rgb_of(palette.text);
        cell.fg = match cell.fg {
            Color::Reset => match text_rgb {
                Some(fg) => to_color(mix(fg, shadow_rgb, 0.72)),
                None => {
                    cell.modifier.insert(Modifier::DIM);
                    Color::Reset
                }
            },
            Color::Rgb(r, g, b) => to_color(mix((r, g, b), shadow_rgb, 0.72)),
            other => {
                cell.modifier.insert(Modifier::DIM);
                other
            }
        };
    }
}

#[inline]
fn strip_backdrop_bits(cell: &mut Cell) {
    let bits = cell.modifier.bits() & !0xf000;
    let mut cleaned = Modifier::from_bits_truncate(bits);
    cleaned.remove(Modifier::BOLD);
    cell.modifier = cleaned;
}

#[inline]
pub(super) fn to_color((r, g, b): (u8, u8, u8)) -> Color {
    Color::Rgb(r, g, b)
}

#[inline]
fn rgb_of(color: Color) -> Option<(u8, u8, u8)> {
    match color {
        Color::Rgb(r, g, b) => Some((r, g, b)),
        _ => None,
    }
}

#[inline]
fn lin(channel: u8) -> f64 {
    let c = f64::from(channel) / 255.0;
    if c <= 0.04045 {
        c / 12.92
    } else {
        ((c + 0.055) / 1.055).powf(2.4)
    }
}

pub(crate) fn luminance((r, g, b): (u8, u8, u8)) -> f64 {
    0.2126 * lin(r) + 0.7152 * lin(g) + 0.0722 * lin(b)
}

pub(crate) fn lstar(rgb: (u8, u8, u8)) -> f64 {
    let y = luminance(rgb);
    if y > 0.008856 {
        116.0 * y.cbrt() - 16.0
    } else {
        903.3 * y
    }
}

pub(crate) fn contrast_ratio(a: (u8, u8, u8), b: (u8, u8, u8)) -> f64 {
    let ya = luminance(a);
    let yb = luminance(b);
    let (hi, lo) = if ya >= yb { (ya, yb) } else { (yb, ya) };
    (hi + 0.05) / (lo + 0.05)
}

#[inline]
pub(super) fn mix(a: (u8, u8, u8), b: (u8, u8, u8), t: f64) -> (u8, u8, u8) {
    let t = t.clamp(0.0, 1.0);
    let blend = |x: u8, y: u8| -> u8 {
        ((1.0 - t) * f64::from(x) + t * f64::from(y))
            .round()
            .clamp(0.0, 255.0) as u8
    };
    (blend(a.0, b.0), blend(a.1, b.1), blend(a.2, b.2))
}

#[inline]
fn scale_rgb((r, g, b): (u8, u8, u8), factor: f64) -> (u8, u8, u8) {
    let sc = |v: u8| -> u8 { (f64::from(v) * factor).round().clamp(0.0, 255.0) as u8 };
    (sc(r), sc(g), sc(b))
}

fn reach_target(from: (u8, u8, u8), to: (u8, u8, u8), target_l: f64, up: bool) -> (u8, u8, u8) {
    let mut lo = 0.0_f64;
    let mut hi = 1.0_f64;
    for _ in 0..24 {
        let mid = 0.5 * (lo + hi);
        let lm = lstar(mix(from, to, mid));
        if (up && lm < target_l) || (!up && lm > target_l) {
            lo = mid;
        } else {
            hi = mid;
        }
    }
    mix(from, to, hi)
}

pub(super) fn move_lstar(
    base: (u8, u8, u8),
    guide: (u8, u8, u8),
    fallback: (u8, u8, u8),
    delta_lstar: f64,
    up: bool,
) -> (u8, u8, u8) {
    let l0 = lstar(base);
    let lf = lstar(fallback);
    let target = if up {
        (l0 + delta_lstar).min(lf)
    } else {
        (l0 - delta_lstar).max(lf)
    };
    let lg = lstar(guide);
    let guide_helps = if up { lg > l0 + 0.5 } else { lg < l0 - 0.5 };
    if guide_helps {
        if (up && lg >= target) || (!up && lg <= target) {
            reach_target(base, guide, target, up)
        } else {
            reach_target(guide, fallback, target, up)
        }
    } else {
        reach_target(base, fallback, target, up)
    }
}

fn pick_raised_dark(
    overlay: (u8, u8, u8),
    surface: (u8, u8, u8),
    selection_bg: Option<(u8, u8, u8)>,
    surface0: (u8, u8, u8),
    text: (u8, u8, u8),
) -> (u8, u8, u8) {
    if let Some(sel) = selection_bg {
        let dl_ov = lstar(sel) - lstar(overlay);
        let dl_sf = lstar(sel) - lstar(surface);
        if (4.5..=10.0).contains(&dl_ov) && dl_sf >= 8.0 {
            return sel;
        }
    }
    let guide = selection_bg.unwrap_or(surface0);
    move_lstar(overlay, guide, text, 5.5, true)
}

fn pick_raised_light(
    surface: (u8, u8, u8),
    selection_bg: Option<(u8, u8, u8)>,
    surface0: (u8, u8, u8),
    text: (u8, u8, u8),
) -> (u8, u8, u8) {
    if let Some(sel) = selection_bg {
        let dl = lstar(surface) - lstar(sel);
        if (4.8..=10.0).contains(&dl) {
            return sel;
        }
    }
    let guide = selection_bg.unwrap_or(surface0);
    move_lstar(surface, guide, text, 5.5, false)
}
