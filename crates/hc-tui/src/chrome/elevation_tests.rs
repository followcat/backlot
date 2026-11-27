//! Verification across every bundled theme: distinct surface/overlay/raised/dim/shadow
//! lightness steps and contrast ratios without hard-coded theme names or RGBs.

use ratatui::buffer::Cell;
use ratatui::style::{Color, Modifier};

use super::elevation::{contrast_ratio, lstar};
use super::palette::Palette;
use crate::config::THEME_NAMES;

fn rgb(color: Color, label: &str, theme: &str) -> (u8, u8, u8) {
    match color {
        Color::Rgb(r, g, b) => (r, g, b),
        other => panic!("{theme} {label}: expected Rgb, got {other:?}"),
    }
}

#[test]
fn every_bundled_rgb_theme_achieves_distinct_elevation_and_legibility() {
    let mut names: Vec<&str> = vec!["backlot", "backlot-day", "backlot-midnight"];
    names.extend(THEME_NAMES.iter().copied());

    for name in names {
        let palette = Palette::from_name(name).expect("valid bundled theme");
        let elev = palette.elevation();

        if name == "terminal" {
            assert!(elev.is_terminal_16);
            assert_eq!(elev.base, Color::Reset);
            assert_eq!(elev.surface, Color::Reset);
            assert_eq!(elev.overlay, Color::Reset);
            assert_eq!(elev.dim_bg, None);
            assert_eq!(elev.shadow, None);
            continue;
        }

        assert!(!elev.is_terminal_16, "{name}: should be RGB elevation");
        let panel_bg = rgb(palette.panel_bg, "panel_bg", name);
        let text = rgb(palette.text, "text", name);
        let surface = rgb(elev.surface, "surface", name);
        let overlay = rgb(elev.overlay, "overlay", name);
        let raised_row = rgb(elev.raised_row, "raised_row", name);
        let raised_field = rgb(elev.raised_field, "raised_field", name);
        let raised_overlay = rgb(elev.raised_on_overlay, "raised_on_overlay", name);
        let dim_bg = rgb(elev.dim_bg.expect("dim_bg"), "dim_bg", name);
        let shadow = rgb(elev.shadow.expect("shadow"), "shadow", name);
        let border_muted = rgb(elev.border_muted, "border_muted", name);
        let muted_surface = rgb(elev.muted_surface, "muted_surface", name);
        let muted_overlay = rgb(elev.muted_overlay, "muted_overlay", name);
        let muted_raised = rgb(elev.muted_raised, "muted_raised", name);

        let dl_base_surf = (lstar(surface) - lstar(panel_bg)).abs();
        let dl_surf_rais = (lstar(raised_row) - lstar(surface)).abs();
        let dl_ovly_field = (lstar(raised_overlay) - lstar(overlay)).abs();
        let dl_dim_ovly = (lstar(overlay) - lstar(dim_bg)).abs();
        let dl_ovly_shdw = (lstar(overlay) - lstar(shadow)).abs();
        let dl_surf_shdw = (lstar(surface) - lstar(shadow)).abs();

        let min_surf = if elev.is_dark { 6.45 } else { 5.45 };
        let min_rais = if elev.is_dark { 7.95 } else { 4.75 };
        // `backlot` Night's `#120f16`/`#0c0a0f` lacquer starts near L* = 4.8,
        // leaving ~12.5 L* between `dim_bg` and `overlay` and ~10.0 L* between
        // `surface` and `shadow`.
        let min_dim = if elev.is_dark { 12.45 } else { 9.45 };
        assert!(
            dl_base_surf >= min_surf,
            "{name}: |ΔL*(base, surface)| = {dl_base_surf:.2} < {min_surf}"
        );
        assert!(
            dl_surf_rais >= min_rais,
            "{name}: |ΔL*(surface, raised_row)| = {dl_surf_rais:.2} < {min_rais}"
        );
        assert!(
            dl_ovly_field >= 4.50,
            "{name}: |ΔL*(overlay, raised_on_overlay)| = {dl_ovly_field:.2} < 4.5"
        );
        assert!(
            dl_dim_ovly >= min_dim,
            "{name}: |ΔL*(dim_bg, overlay)| = {dl_dim_ovly:.2} < {min_dim}"
        );
        assert!(
            dl_ovly_shdw >= 13.10,
            "{name}: |ΔL*(overlay, shadow)| = {dl_ovly_shdw:.2} < 13.10"
        );
        assert!(
            dl_surf_shdw >= 9.95,
            "{name}: |ΔL*(surface, shadow)| = {dl_surf_shdw:.2} < 9.95"
        );
        assert!(
            (lstar(raised_field) - lstar(surface)).abs() >= 4.95,
            "{name}: |ΔL*(surface, raised_field)| < 5.0"
        );

        let cr_base = contrast_ratio(text, panel_bg);
        let cr_surf = contrast_ratio(text, surface);
        let cr_ovly = contrast_ratio(text, overlay);
        let soft = matches!(name, "solarized" | "solarized-light" | "tokyo-night-day");
        let min_surf_cr = if soft { 3.55 } else { 4.50 };
        let min_ovly_cr = if soft { 4.05 } else { 4.50 };
        assert!(
            cr_surf >= min_surf_cr && (cr_surf >= 7.0 || cr_surf >= 0.80 * cr_base),
            "{name}: CR(text, surface)={cr_surf:.2} (baseline {cr_base:.2}, min {min_surf_cr})"
        );
        assert!(
            cr_ovly >= min_ovly_cr && (cr_ovly >= 7.0 || cr_ovly >= 0.72 * cr_base),
            "{name}: CR(text, overlay)={cr_ovly:.2} (baseline {cr_base:.2}, min {min_ovly_cr})"
        );
        assert!(
            contrast_ratio(muted_surface, surface) >= 2.98,
            "{name}: muted_surface CR too low"
        );
        assert!(
            contrast_ratio(muted_overlay, overlay) >= 2.98,
            "{name}: muted_overlay CR too low"
        );
        assert!(
            contrast_ratio(muted_raised, raised_field) >= 2.98,
            "{name}: muted_raised CR too low"
        );
        assert!(
            contrast_ratio(border_muted, surface) >= 1.38,
            "{name}: border_muted CR={:.2} < 1.4",
            contrast_ratio(border_muted, surface)
        );
    }
}

#[test]
fn dim_cell_attenuates_rgb_fills_reset_and_falls_back_to_dim_for_indexed() {
    let p = Palette::backlot();
    let elev = p.elevation();

    let mut rgb_cell = Cell::new("a");
    rgb_cell.set_fg(p.text).set_bg(p.panel_bg);
    rgb_cell.modifier = Modifier::BOLD;
    elev.dim_cell(&mut rgb_cell, &p);
    assert_ne!(rgb_cell.fg, p.text);
    assert_ne!(rgb_cell.bg, p.panel_bg);
    assert!(!rgb_cell.modifier.contains(Modifier::BOLD));
    assert!(!rgb_cell.modifier.contains(Modifier::DIM));

    let mut reset_cell = Cell::new("b");
    elev.dim_cell(&mut reset_cell, &p);
    assert_eq!(Some(reset_cell.bg), elev.dim_bg);
    assert_ne!(reset_cell.fg, Color::Reset);

    let mut idx_cell = Cell::new("c");
    idx_cell
        .set_fg(Color::Indexed(11))
        .set_bg(Color::Indexed(4));
    elev.dim_cell(&mut idx_cell, &p);
    assert_eq!(idx_cell.fg, Color::Indexed(11));
    assert_eq!(idx_cell.bg, Color::Indexed(4));
    assert!(idx_cell.modifier.contains(Modifier::DIM));

    let term = Palette::terminal();
    let term_elev = term.elevation();
    let mut term_cell = Cell::new("t");
    term_elev.dim_cell(&mut term_cell, &term);
    assert_eq!(term_cell.bg, Color::Reset);
    assert!(term_cell.modifier.contains(Modifier::DIM));

    let mut sh_cell = Cell::new("s");
    sh_cell.set_fg(p.text).set_bg(p.panel_bg);
    elev.shadow_cell(&mut sh_cell, &p);
    assert_eq!(Some(sh_cell.bg), elev.shadow);
    assert_ne!(sh_cell.fg, p.text);
    assert_eq!(sh_cell.symbol(), "s");

    let mut term_sh = Cell::new("x");
    term_sh.set_fg(Color::Gray).set_bg(Color::Reset);
    term_elev.shadow_cell(&mut term_sh, &term);
    assert_eq!(term_sh.bg, Color::Reset);
    assert_eq!(term_sh.fg, Color::Gray);
}
