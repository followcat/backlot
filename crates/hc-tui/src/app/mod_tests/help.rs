//! S0's help additions: backlot's own sections, the studio keys'
//! shadow notes and the two-line glyph legend — studio mode only;
//! `--plain` keeps `Ctrl+B ?` cell-identical to herdr's.
use super::*;
use studio::two_studio_app;

#[tokio::test]
async fn studio_mode_lists_backlot_sections_shadow_notes_and_the_legend() {
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
    // The backlot sections sit at the list's bottom — the last page
    // shows the studio keys and the legend.
    app.help_state.scroll = usize::MAX;
    let text = screen_lines(&draw_to(&mut app, 140, 40)).join("\n");
    assert!(text.contains("call sheet"), "{text}");
    assert!(text.contains("previous studio"), "{text}");
    assert!(text.contains("next role"), "{text}");
    assert!(text.contains("shadows herdr:"), "the shadow note: {text}");
    assert!(text.contains("glyph legend"), "{text}");
    assert!(text.contains("◐ work"), "the people row: {text}");
    assert!(text.contains("· wait"), "the tasks row: {text}");
    // A few rows up: the "backlot" header and the dock chord.
    app.help_state.scroll = app.help_hits.max_scroll.saturating_sub(4);
    let text = screen_lines(&draw_to(&mut app, 140, 40)).join("\n");
    assert!(text.contains("backlot"), "{text}");
    assert!(text.contains("studio dock"), "{text}");
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
    assert!(!text.contains("previous studio"), "{text}");
    // Herdr's own sections are untouched.
    assert!(text.contains("navigation"), "{text}");
    assert!(text.contains("keybinds"), "{text}");
}

#[tokio::test]
async fn an_unshadowed_studio_key_carries_no_note() {
    let mut app = two_studio_app();
    app.help = true;
    app.help_state.scroll = usize::MAX;
    let text = screen_lines(&draw_to(&mut app, 140, 40)).join("\n");
    assert!(text.contains("previous studio"), "{text}");
    // The default config binds nothing on alt+arrows — no note.
    assert!(!text.contains("shadows"), "{text}");
}
