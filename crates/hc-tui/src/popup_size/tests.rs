// Derived from herdr src/popup_size.rs tests (Apache-2.0).
use super::*;
use ratatui::layout::Rect;

#[test]
fn percent_resolves_against_available() {
    assert_eq!(PopupSize::Percent(80).resolve(100), 80);
    assert_eq!(PopupSize::Percent(40).resolve(30), 12);
    assert_eq!(PopupSize::Cells(17).resolve(100), 17);
}

#[test]
fn default_is_half_the_surface_with_minimums() {
    let geometry = resolve_popup_geometry(None, None, Rect::new(0, 0, 100, 30)).unwrap();
    assert_eq!(geometry.outer, Rect::new(25, 7, 50, 15));
    assert_eq!(geometry.inner, Rect::new(26, 8, 48, 13));
    assert_eq!(geometry.terminal_cols, 47);
    assert_eq!(geometry.terminal_rows, 13);
}

#[test]
fn reference_example_80x40_percent_on_100x30() {
    let geometry = resolve_popup_geometry(
        Some(PopupSize::Percent(80)),
        Some(PopupSize::Percent(40)),
        Rect::new(0, 0, 100, 30),
    )
    .unwrap();
    assert_eq!(geometry.outer, Rect::new(10, 9, 80, 12));
    assert_eq!(geometry.inner, Rect::new(11, 10, 78, 10));
}

#[test]
fn requests_clamp_to_the_surface_and_minimums() {
    let geometry = resolve_popup_geometry(
        Some(PopupSize::Cells(500)),
        Some(PopupSize::Cells(1)),
        Rect::new(0, 0, 60, 20),
    )
    .unwrap();
    assert_eq!(geometry.outer, Rect::new(0, 8, 60, 4));
    let tiny = resolve_popup_geometry(
        Some(PopupSize::Cells(1)),
        Some(PopupSize::Cells(1)),
        Rect::new(0, 0, 20, 10),
    )
    .unwrap();
    assert_eq!(tiny.outer, Rect::new(7, 3, 6, 4));
    assert_eq!(tiny.terminal_cols, 4);
}

#[test]
fn too_small_a_surface_has_no_popup() {
    assert_eq!(
        resolve_popup_geometry(None, None, Rect::new(0, 0, 5, 3)),
        None
    );
}

#[test]
fn toml_values_parse() {
    assert_eq!(
        PopupSize::from_toml(&toml::Value::String("80%".into())),
        Ok(PopupSize::Percent(80))
    );
    assert_eq!(
        PopupSize::from_toml(&toml::Value::Integer(24)),
        Ok(PopupSize::Cells(24))
    );
    assert!(PopupSize::from_toml(&toml::Value::String("0%".into())).is_err());
    assert!(PopupSize::from_toml(&toml::Value::String("80".into())).is_err());
    assert!(PopupSize::from_toml(&toml::Value::Boolean(true)).is_err());
}
