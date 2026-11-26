//! S0's help additions: backlot's own sections first, the studio
//! keys' shadow notes and the two-line glyph legend pinned above the
//! footer — studio mode only; `--plain` keeps `Ctrl+B ?`
//! cell-identical to herdr's.
use super::*;
use studio::two_studio_app;

#[tokio::test]
async fn studio_mode_leads_with_backlot_sections_and_pins_the_legend() {
    let mut app = two_studio_app();
    // An alt+up previous_agent binding makes the shadow note real.
    app.live
        .keybinds
        .previous_agent
        .bindings
        .push(crate::config::ResolvedBinding {
            trigger: crate::config::BindingTrigger::Direct((
                crossterm::event::KeyCode::Up,
                crossterm::event::KeyModifiers::ALT,
            )),
            label: "alt+up".into(),
        });
    app.help = true;
    // S5's launcher leads with its action groups, the backlot
    // reference sections follow — and the legend stays pinned.
    let text = screen_lines(&draw_to(&mut app, 140, 40)).join("\n");
    assert!(text.contains("Actions & Keybinds"), "{text}");
    assert!(text.contains("studio & task actions"), "{text}");
    assert!(text.contains("previous studio"), "{text}");
    assert!(text.contains("shadows herdr:"), "the shadow note: {text}");
    // The backlot reference sections keep their titles and rows
    // below the fold — the model carries them even off-screen.
    let groups = app.help_groups();
    assert!(
        groups
            .iter()
            .flat_map(|g| g.rows.iter())
            .any(|r| r.label == "next role"),
        "next role is a row"
    );
    let titles: Vec<String> = groups.into_iter().map(|g| g.title).collect();
    for title in ["call sheet (open)", "studio dock (open)", "studio dialogs"] {
        assert!(titles.iter().any(|t| t == title), "{title}: {titles:?}");
    }
    assert!(text.contains("◐ work"), "the people row: {text}");
    assert!(text.contains("· wait"), "the tasks row: {text}");
    // Scrolled to the bottom and mid-filter, the legend stays pinned.
    app.help_state.scroll = usize::MAX;
    let text = screen_lines(&draw_to(&mut app, 140, 40)).join("\n");
    assert!(text.contains("◐ work"), "scrolled: {text}");
    assert!(text.contains("· wait"), "scrolled: {text}");
    app.help_state.query.insert("zzz-no-match");
    let text = screen_lines(&draw_to(&mut app, 140, 40)).join("\n");
    assert!(text.contains("◐ work"), "filtered: {text}");
    assert!(text.contains("· wait"), "filtered: {text}");
}

#[tokio::test]
async fn plain_keeps_the_help_cell_identical() {
    let mut app = two_studio_app();
    app.set_plain(true);
    app.help = true;
    let text = screen_lines(&draw_to(&mut app, 140, 40)).join("\n");
    assert!(!text.contains("backlot"), "{text}");
    assert!(!text.contains("studio dock"), "{text}");
    assert!(!text.contains("glyph legend"), "{text}");
    assert!(!text.contains("◐ work"), "{text}");
    assert!(!text.contains("previous studio"), "{text}");
    // Herdr's own sections are untouched.
    assert!(text.contains("navigation"), "{text}");
    assert!(text.contains("keybinds"), "{text}");
}

#[tokio::test]
async fn an_unshadowed_studio_key_carries_no_note() {
    let mut app = two_studio_app();
    app.help = true;
    let text = screen_lines(&draw_to(&mut app, 140, 40)).join("\n");
    assert!(text.contains("previous studio"), "{text}");
    // The default config binds nothing on alt+arrows — no note.
    assert!(!text.contains("shadows"), "{text}");
}

/// R3-N6: every pinned glyph-legend line stays ≤ 66 cells — inside
/// the 80×24 overlay's 74 inner columns `✓ closed` never clips.
#[tokio::test]
async fn the_glyph_legend_fits_inside_eighty_columns() {
    let mut app = two_studio_app();
    app.help = true;
    let text = screen_lines(&draw_to(&mut app, 80, 24)).join("\n");
    assert!(text.contains("◐ work"), "{text}");
    assert!(text.contains("✓ closed"), "{text}");
    assert!(
        app.help_legend()
            .iter()
            .any(|(key, label)| key == "roles" && label.contains("Ⓟ producer")),
        "the role legend line is pinned"
    );
    // The renderer's ` {key:<7} {label}` at ≤66 cells each.
    for (key, label) in app.help_legend() {
        let line = format!(" {key:<7} {label}");
        assert!(
            crate::chrome::text::display_width(&line) <= 66,
            "{line:?} wider than 66"
        );
    }
}
