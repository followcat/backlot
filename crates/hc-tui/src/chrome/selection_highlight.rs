// Derived from herdr src/ui/panes.rs and src/ui/widgets.rs (Apache-2.0).
//! The selection highlight paint. Selected cells take one computed style —
//! the palette/host-derived selection background plus a contrast-chosen
//! foreground — applied over the pane like the reference's `set_style`.

use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::{Color, Style};

use super::palette::Palette;
use super::scroll::ScrollMetrics;
use crate::host::HostTheme;
use crate::selection::Selection;

/// Paint the visible selection for one pane — the reference's
/// `render_selection_highlight`. `inner` is the pane content rect,
/// `scroll` the hit's viewport metrics.
pub(crate) fn render_selection_highlight(
    selection: Option<&Selection>,
    buffer: &mut Buffer,
    pane_id: &str,
    inner: Rect,
    scroll: Option<ScrollMetrics>,
    p: &Palette,
    host_theme: &HostTheme,
) {
    let Some(selection) =
        selection.filter(|selection| selection.is_visible() && selection.pane_id == pane_id)
    else {
        return;
    };
    let style = selection_style(p, host_theme);
    for y in 0..inner.height {
        for x in 0..inner.width {
            if selection.contains(y, x, scroll) {
                buffer[(inner.x + x, inner.y + y)].set_style(style);
            }
        }
    }
}

type Rgb = (u8, u8, u8);

/// The one computed selection style — shared by pane and popup
/// selection paint.
pub(crate) fn selection_style(p: &Palette, host_theme: &HostTheme) -> Style {
    let bg = automatic_selection_bg(p, host_theme);
    Style::reset().fg(selection_fg_for_bg(bg, p)).bg(bg)
}

fn automatic_selection_bg(p: &Palette, host_theme: &HostTheme) -> Color {
    let fallback = selection_palette_background(p);
    let Some(background) = host_theme
        .background
        .map(|color| (color.r, color.g, color.b))
        .or(match fallback {
            Color::Rgb(r, g, b) => Some((r, g, b)),
            _ => None,
        })
    else {
        return fallback;
    };

    let target = if relative_luminance(background) < 0.5 {
        (255, 255, 255)
    } else {
        (0, 0, 0)
    };
    let selected = mix_rgb(background, target, 0.28);
    Color::Rgb(selected.0, selected.1, selected.2)
}

fn selection_palette_background(p: &Palette) -> Color {
    if p.panel_bg == Color::Reset {
        p.surface_dim
    } else {
        p.panel_bg
    }
}

fn selection_fg_for_bg(bg: Color, p: &Palette) -> Color {
    if let Color::Rgb(r, g, b) = bg {
        let luminance = relative_luminance((r, g, b));
        let black_contrast = (luminance + 0.05) / 0.05;
        let white_contrast = 1.05 / (luminance + 0.05);
        return if black_contrast > white_contrast {
            Color::Rgb(0, 0, 0)
        } else {
            Color::Rgb(255, 255, 255)
        };
    }

    color_to_rgb(bg)
        .map(|bg| {
            if relative_luminance(bg) < 0.5 {
                Color::White
            } else {
                Color::Black
            }
        })
        .unwrap_or_else(|| panel_contrast_fg(p))
}

fn mix_rgb(base: Rgb, target: Rgb, amount: f32) -> Rgb {
    fn channel(base: u8, target: u8, amount: f32) -> u8 {
        (f32::from(base) + (f32::from(target) - f32::from(base)) * amount).round() as u8
    }
    (
        channel(base.0, target.0, amount),
        channel(base.1, target.1, amount),
        channel(base.2, target.2, amount),
    )
}

fn relative_luminance(color: Rgb) -> f32 {
    fn channel(value: u8) -> f32 {
        let value = f32::from(value) / 255.0;
        if value <= 0.03928 {
            value / 12.92
        } else {
            ((value + 0.055) / 1.055).powf(2.4)
        }
    }
    0.2126 * channel(color.0) + 0.7152 * channel(color.1) + 0.0722 * channel(color.2)
}

fn color_to_rgb(color: Color) -> Option<Rgb> {
    match color {
        Color::Reset => None,
        Color::Black => Some((0, 0, 0)),
        Color::Red => Some((128, 0, 0)),
        Color::Green => Some((0, 128, 0)),
        Color::Yellow => Some((128, 128, 0)),
        Color::Blue => Some((0, 0, 128)),
        Color::Magenta => Some((128, 0, 128)),
        Color::Cyan => Some((0, 128, 128)),
        Color::Gray => Some((192, 192, 192)),
        Color::DarkGray => Some((128, 128, 128)),
        Color::LightRed => Some((255, 0, 0)),
        Color::LightGreen => Some((0, 255, 0)),
        Color::LightYellow => Some((255, 255, 0)),
        Color::LightBlue => Some((0, 0, 255)),
        Color::LightMagenta => Some((255, 0, 255)),
        Color::LightCyan => Some((0, 255, 255)),
        Color::White => Some((255, 255, 255)),
        Color::Rgb(r, g, b) => Some((r, g, b)),
        Color::Indexed(_) => None,
    }
}

/// The reference's `panel_contrast_fg` — the palette fallback when the
/// selection background isn't a resolvable color.
fn panel_contrast_fg(palette: &Palette) -> Color {
    match palette.panel_bg {
        Color::Reset => palette.surface_dim,
        color => color,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn palette(panel_bg: Color, surface_dim: Color) -> Palette {
        let mut p = Palette::terminal();
        p.panel_bg = panel_bg;
        p.surface_dim = surface_dim;
        p
    }

    #[test]
    fn dark_host_background_lifts_toward_white() {
        let p = palette(Color::Reset, Color::Reset);
        let host = HostTheme {
            background: Some(hc_vt::Rgb {
                r: 12,
                g: 14,
                b: 16,
            }),
            ..Default::default()
        };
        let Color::Rgb(r, g, b) = automatic_selection_bg(&p, &host) else {
            panic!("expected an rgb selection bg")
        };
        assert!(relative_luminance((r, g, b)) > relative_luminance((12, 14, 16)));
        assert_eq!(
            selection_fg_for_bg(Color::Rgb(r, g, b), &p),
            Color::Rgb(255, 255, 255)
        );
    }

    #[test]
    fn light_host_background_dips_toward_black() {
        let p = palette(Color::Reset, Color::Reset);
        let host = HostTheme {
            background: Some(hc_vt::Rgb {
                r: 245,
                g: 245,
                b: 245,
            }),
            ..Default::default()
        };
        let Color::Rgb(r, g, b) = automatic_selection_bg(&p, &host) else {
            panic!("expected an rgb selection bg")
        };
        assert!(relative_luminance((r, g, b)) < relative_luminance((245, 245, 245)));
        assert_eq!(
            selection_fg_for_bg(Color::Rgb(r, g, b), &p),
            Color::Rgb(0, 0, 0)
        );
    }

    #[test]
    fn palette_fallback_uses_panel_bg_then_surface_dim() {
        let p = palette(Color::Rgb(10, 20, 30), Color::Rgb(1, 2, 3));
        assert_eq!(selection_palette_background(&p), Color::Rgb(10, 20, 30));
        let p = palette(Color::Reset, Color::Rgb(1, 2, 3));
        assert_eq!(selection_palette_background(&p), Color::Rgb(1, 2, 3));
    }
}
