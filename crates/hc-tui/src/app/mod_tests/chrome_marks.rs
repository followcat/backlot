//! S0 chrome and W1's badge on one row: the studio header (or the tab
//! bar), the lot pill, the version badge and the launcher must share
//! the top row at every supported width without overlapping or
//! clipping each other.
use super::*;
use crate::panel::PanelMode;

fn version() -> String {
    format!("v{}", env!("CARGO_PKG_VERSION"))
}

/// A studio app whose top row carries everything at once: the badge in
/// the sidebar header, the studio header over the tab bar, the lot
/// pill at the row's right end.
fn studio_app() -> App {
    let mut app = super::studio_machines::two_machine_studio_app();
    app.update_latest = Some("9.9.9".into());
    app
}

fn row0(app: &mut App, w: u16, h: u16) -> (String, TestBackend) {
    app.area = (w, h);
    let backend = draw_to(app, w, h);
    (screen_lines(&backend)[0].clone(), backend)
}

/// The badge, the studio header and the pill share row 0 at 80, 120
/// and 160 columns: each string is drawn whole, their cells never
/// overlap, and the pill keeps its right-hand segment.
#[tokio::test]
async fn the_badge_the_studio_header_and_the_pill_never_overlap() {
    for w in [80u16, 120, 160] {
        let mut app = studio_app();
        let (header, _) = row0(&mut app, w, 24);
        assert!(header.contains("machines"), "{w}: {header:?}");
        assert!(
            header.contains(&version()),
            "{w}: badge missing: {header:?}"
        );
        assert!(header.contains("↑v9.9.9"), "{w}: hint missing: {header:?}");
        assert!(header.contains("acme-shop"), "{w}: header: {header:?}");
        assert!(header.contains('✦'), "{w}: pill missing: {header:?}");
        let badge = app.hit_map.update_badge;
        let pill = app.hit_map.lot_pill;
        let name = app.hit_map.studio_header;
        assert_eq!(badge.y, 0, "{w}: badge row");
        assert_eq!(pill.y, 0, "{w}: pill row");
        assert_eq!(name.y, 0, "{w}: studio header row");
        assert!(badge.right() <= name.x, "{w}: badge over the header");
        assert!(name.right() <= pill.x, "{w}: header over the pill");
        assert_eq!(pill.right(), w, "{w}: the pill ends the row");
        // The version text sits inside the badge's own run, and the
        // hint's hit covers the hint only.
        let text = header.find(&version()).unwrap() as u16;
        assert!(text + version().len() as u16 <= badge.x, "{w}: {header:?}");
        assert!(badge.x >= 10, "{w}: the header label stays: {header:?}");
    }
}

/// A studio workspace draws the studio header instead of the tab bar —
/// and remembered chrome never brings the tab bar back.
#[tokio::test]
async fn a_studio_workspace_keeps_the_studio_header_over_the_tab_bar() {
    let mut app = studio_app();
    let (header, _) = row0(&mut app, 120, 24);
    assert!(header.contains("acme-shop"), "{header:?}");
    assert!(app.hit_map.tabs.is_empty(), "the tab bar is gone");
    assert!(!app.hit_map.studio_header.is_empty());

    // Remembered chrome: a collapsed sidebar, a dragged width, a split
    // and a saved sort are all restored before a studio workspace
    // draws — none of them may reinstate the tab bar.
    let mut app = studio_app();
    app.shell_state.sidebar_collapsed = true;
    app.sidebar_width = 30;
    app.shell_state.sidebar_section_split = 0.7;
    app.shell.agent_panel_sort = crate::config::AgentPanelSortConfig::Spaces;
    let (header, _) = row0(&mut app, 120, 24);
    assert!(header.contains("acme-shop"), "{header:?}");
    assert!(app.hit_map.tabs.is_empty(), "the tab bar came back");
    let mut app = studio_app();
    app.shell_state.sidebar_collapsed = true;
    let (header, _) = row0(&mut app, 80, 24);
    assert!(header.contains("acme-shop"), "{header:?}");
    assert!(app.hit_map.tabs.is_empty(), "the tab bar came back");
}

/// The mobile header at 60 and 40 columns keeps the studio name whole,
/// plus the badge and the pill.
#[tokio::test]
async fn the_mobile_header_keeps_the_name_the_badge_and_the_pill() {
    for w in [60u16, 40] {
        let mut app = studio_app();
        let (header, _) = row0(&mut app, w, 24);
        assert!(
            header.contains("acme-shop"),
            "{w}: name clipped: {header:?}"
        );
        assert!(
            header.contains(&version()),
            "{w}: badge missing: {header:?}"
        );
        assert!(header.contains('✦'), "{w}: pill missing: {header:?}");
        assert!(app.hit_map.mobile_switch.width > 0, "{w}: switch");
        let badge = app.hit_map.update_badge;
        assert_eq!(badge.y, 0, "{w}: badge row");
        assert!(
            badge.right() <= app.hit_map.lot_pill.x,
            "{w}: badge over the pill"
        );
    }
}

/// Outside a studio workspace the pill rides the tab bar's right end,
/// next to the tab scroll controls — never over them and never clipped
/// by them, at 80, 120 and 160 columns.
#[tokio::test]
async fn the_pill_keeps_the_tab_bars_right_end_outside_a_studio_workspace() {
    for w in [80u16, 120, 160] {
        let mut app = studio_app();
        // The stray agent in the `backlot` space is no studio workspace,
        // so this workspace keeps herdr's tab bar with the pill on it.
        app.focused = Some("t50".into());
        app.reconcile_published_view();
        let (row, _) = row0(&mut app, w, 24);
        assert!(!app.hit_map.tabs.is_empty(), "{w}: tab bar: {row:?}");
        assert!(row.contains(&version()), "{w}: badge: {row:?}");
        assert!(row.contains('✦'), "{w}: pill missing: {row:?}");
        let pill = app.hit_map.lot_pill;
        assert_eq!(pill.y, 0, "{w}: pill row");
        assert_eq!(pill.right(), w, "{w}: the pill ends the row");
        assert!(
            app.hit_map.update_badge.right() <= pill.x,
            "{w}: badge over the pill"
        );
        for (rect, _) in &app.hit_map.tabs {
            assert!(rect.right() <= pill.x, "{w}: a tab lies under the pill");
        }
    }
}

/// R3-M6/C1: over a non-studio workspace (`backlot`'s stray agent) the
/// remembered current studio stops painting as active — the dock
/// block loses its `▸` — while the cast still names it and
/// `studio.current` stays remembered for re-entry.
#[tokio::test]
async fn a_non_studio_workspace_clears_the_dock_mark_but_keeps_the_cast() {
    fn regions(app: &mut App, w: u16, h: u16) -> (String, String) {
        let backend = draw_to(app, w, h);
        let buf = backend.buffer();
        let side_w = app.sidebar_render_width(w);
        let sidebar = (0..h)
            .map(|y| {
                (0..side_w)
                    .map(|x| buf[(x, y)].symbol())
                    .collect::<String>()
            })
            .collect::<Vec<_>>()
            .join("\n");
        let area = app.panel_hits.area;
        let dock = (area.y..area.bottom())
            .map(|y| {
                (area.x..area.right())
                    .map(|x| buf[(x, y)].symbol())
                    .collect::<String>()
            })
            .collect::<Vec<_>>()
            .join("\n");
        (sidebar, dock)
    }

    let mut app = studio_app();
    app.panel.mode = PanelMode::Expanded;
    assert!(app.studio.lot.studio_workspace, "fixture focuses t1");
    // In the studio workspace: `▸` on the dock block, the cast rows
    // in the sidebar.
    let (sidebar, dock) = regions(&mut app, 120, 40);
    assert!(dock.contains('▸'), "in-studio mark: {dock}");
    assert!(sidebar.contains("acme-shop · Local"), "cast: {sidebar}");
    assert!(sidebar.contains("producer"), "cast row: {sidebar}");

    // The stray `backlot` agent is no studio workspace: the blocks
    // stay, `▸` goes — and the cast keeps naming the remembered
    // current studio (C1: the left side explains itself).
    app.focused = Some("t50".into());
    app.reconcile_published_view();
    assert!(!app.studio.lot.studio_workspace);
    assert!(
        app.studio.current.is_some(),
        "the studio stays remembered for Alt+Right re-entry"
    );
    let (sidebar, dock) = regions(&mut app, 120, 40);
    assert!(!dock.contains('▸'), "mark cleared: {dock}");
    assert!(dock.contains("acme-shop"), "the blocks stay: {dock}");
    assert!(
        sidebar.contains("acme-shop · Local"),
        "the remembered current studio's cast stays: {sidebar}"
    );
    assert!(sidebar.contains("producer"), "cast row stays: {sidebar}");
}
