//! The sidebar header's version badge: the running `vX.Y.Z`, plus the
//! update hint that degrades in steps as the header narrows — the full
//! ` ↑v…`/` ↻v…`, then a compact ` ↑`/` ↻`, then nothing, then the whole
//! badge. The hint is the clickable slice that opens the update card.

use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::{Color, Modifier, Style};

use super::palette::Palette;
use super::render::{display_width, put_text};

/// The version badge's update hint — ` ↑v…` in accent while a newer
/// release is offered, ` ↻v…` in green once a verified install only
/// waits on its restart. The restart hint wins while both are known.
#[derive(Clone, Copy)]
pub(crate) enum UpdateBadge<'a> {
    None,
    Update(&'a str),
    Restart(&'a str),
}

impl<'a> UpdateBadge<'a> {
    pub(crate) fn new(update: Option<&'a str>, restart: Option<&'a str>) -> Self {
        match (restart, update) {
            (Some(version), _) => Self::Restart(version),
            (None, Some(version)) => Self::Update(version),
            (None, None) => Self::None,
        }
    }
}

impl UpdateBadge<'_> {
    /// The accent of the update dot and the collapsed toggle: accent while
    /// an update waits, green once an install waits for its restart.
    pub(crate) fn accent_style(self, palette: &Palette) -> Option<Style> {
        match self {
            UpdateBadge::None => None,
            UpdateBadge::Update(_) => Some(
                Style::default()
                    .fg(palette.accent)
                    .add_modifier(Modifier::BOLD),
            ),
            UpdateBadge::Restart(_) => Some(
                Style::default()
                    .fg(palette.green)
                    .add_modifier(Modifier::BOLD),
            ),
        }
    }
}

/// The running version as it draws, `v<X.Y.Z>`.
fn running_version() -> String {
    format!("v{}", env!("CARGO_PKG_VERSION"))
}

/// The full update hint — ` ↑v<latest>` for a newer release, ` ↻v<version>`
/// once a verified install waits on its restart, `None` otherwise. It
/// stays tight so a two-digit pair still fits the default-width header.
fn full_hint(update_badge: UpdateBadge<'_>) -> Option<String> {
    match update_badge {
        UpdateBadge::Update(latest) => Some(format!(" ↑v{latest}")),
        UpdateBadge::Restart(version) => Some(format!(" ↻v{version}")),
        UpdateBadge::None => None,
    }
}

/// The compact one-glyph hint that replaces the full one under width
/// pressure — same colour, still the clickable slice, so the update
/// card stays one click away on a narrow sidebar.
fn compact_hint(update_badge: UpdateBadge<'_>) -> Option<&'static str> {
    match update_badge {
        UpdateBadge::Update(_) => Some(" ↑"),
        UpdateBadge::Restart(_) => Some(" ↻"),
        UpdateBadge::None => None,
    }
}

fn hint_color(update_badge: UpdateBadge<'_>, palette: &Palette) -> Color {
    match update_badge {
        UpdateBadge::Restart(_) => palette.green,
        _ => palette.accent,
    }
}

/// The badge widths, widest first: the full hint, the compact hint,
/// then the version alone. Without an update all three coincide.
fn badge_widths(version: &str, update_badge: UpdateBadge<'_>) -> [u16; 3] {
    let ver_w = display_width(version);
    match (full_hint(update_badge), compact_hint(update_badge)) {
        (Some(full), Some(compact)) => [
            ver_w + display_width(&full),
            ver_w + display_width(compact),
            ver_w,
        ],
        _ => [ver_w, ver_w, ver_w],
    }
}

/// Width of the fully drawn badge — callers reserve a slot with it.
pub(super) fn version_badge_width(update_badge: UpdateBadge<'_>) -> u16 {
    badge_widths(&running_version(), update_badge)[0]
}

/// Width of the badge carrying only the compact ` ↑`/` ↻` hint.
pub(super) fn version_badge_compact_width(update_badge: UpdateBadge<'_>) -> u16 {
    badge_widths(&running_version(), update_badge)[1]
}

/// Right-aligns `version` plus its hint inside `row` without crossing
/// `min_left`, degrading in steps: the full hint gives way to the
/// compact one, then to no hint, then the whole badge goes. Returns
/// the drawn hint's rect — the clickable slice — empty when none.
fn put_badge(
    buffer: &mut Buffer,
    row: Rect,
    min_left: u16,
    version: &str,
    update_badge: UpdateBadge<'_>,
    palette: &Palette,
) -> Rect {
    put_badge_styled(
        buffer,
        row,
        min_left,
        version,
        update_badge,
        palette,
        palette.overlay0,
        None,
    )
}

#[allow(clippy::too_many_arguments)]
fn put_badge_styled(
    buffer: &mut Buffer,
    row: Rect,
    min_left: u16,
    version: &str,
    update_badge: UpdateBadge<'_>,
    palette: &Palette,
    ver_fg: Color,
    ver_bg: Option<Color>,
) -> Rect {
    let ver_w = display_width(version);
    let room = row.right().saturating_sub(min_left);
    // The widest hint this row has room for: full, compact, or none.
    let hint = match (full_hint(update_badge), compact_hint(update_badge)) {
        (Some(full), _) if room >= ver_w + display_width(&full) => full,
        (Some(_), Some(compact)) if room >= ver_w + display_width(compact) => compact.to_string(),
        _ => String::new(),
    };
    let hint_w = display_width(&hint);
    if room < ver_w + hint_w {
        return Rect::default();
    }
    let ver_x = row.right().saturating_sub(ver_w + hint_w);
    let mut ver_style = Style::default().fg(ver_fg);
    if let Some(bg) = ver_bg {
        ver_style = ver_style.bg(bg);
    }
    put_text(buffer, ver_x, row.y, ver_w, version, ver_style);
    if hint.is_empty() {
        return Rect::default();
    }
    let hint_x = ver_x.saturating_add(ver_w);
    let mut h_style = Style::default().fg(hint_color(update_badge, palette));
    if let Some(bg) = ver_bg {
        h_style = h_style.bg(bg);
    }
    put_text(buffer, hint_x, row.y, hint_w, &hint, h_style);
    Rect::new(hint_x, row.y, hint_w, 1)
}

/// Right-aligns the running version plus its update hint — see
/// `put_badge` for the degrade order.
pub(super) fn put_version_badge(
    buffer: &mut Buffer,
    row: Rect,
    min_left: u16,
    update_badge: UpdateBadge<'_>,
    palette: &Palette,
) -> Rect {
    put_badge(
        buffer,
        row,
        min_left,
        &running_version(),
        update_badge,
        palette,
    )
}

pub(super) fn put_version_badge_styled(
    buffer: &mut Buffer,
    row: Rect,
    min_left: u16,
    update_badge: UpdateBadge<'_>,
    palette: &Palette,
    ver_fg: Color,
    ver_bg: Option<Color>,
) -> Rect {
    put_badge_styled(
        buffer,
        row,
        min_left,
        &running_version(),
        update_badge,
        palette,
        ver_fg,
        ver_bg,
    )
}

#[cfg(test)]
mod badge_tests {
    use super::*;

    fn row_text(buf: &Buffer, width: u16) -> String {
        (0..width)
            .map(|x| buf[(x, 0)].symbol())
            .collect::<String>()
            .trim()
            .to_owned()
    }

    /// Draw `version` plus its hint in a 14-cell header — the studio
    /// sidebar's room. Returns the drawn text and the hint's hit rect.
    fn draw(version: &str, badge: UpdateBadge<'_>) -> (String, Rect) {
        let palette = Palette::catppuccin();
        let mut buf = Buffer::empty(Rect::new(0, 0, 14, 1));
        let hit = put_badge(
            &mut buf,
            Rect::new(0, 0, 14, 1),
            0,
            version,
            badge,
            &palette,
        );
        (row_text(&buf, 14), hit)
    }

    #[test]
    fn the_full_hint_shows_while_it_fits() {
        // `v0.4.9 ↑v9.9.9` is exactly 14 cells — the whole header.
        let (text, hit) = draw("v0.4.9", UpdateBadge::Update("9.9.9"));
        assert_eq!(text, "v0.4.9 ↑v9.9.9");
        // The hit covers exactly the drawn ` ↑v9.9.9`, not the version.
        assert_eq!(hit, Rect::new(6, 0, 8, 1));
    }

    #[test]
    fn a_long_version_degrades_to_the_compact_hint() {
        // 0.4.10-style bump: `v0.4.10 ↑v9.9.9` is 15 cells, one too
        // many for the header — the compact ` ↑` keeps the hint alive.
        let (text, hit) = draw("v0.4.10", UpdateBadge::Update("9.9.9"));
        assert_eq!(text, "v0.4.10 ↑");
        // Right-aligned: `v0.4.10` in columns 5..12, ` ↑` in 12..14.
        assert_eq!(hit, Rect::new(12, 0, 2, 1));
    }

    #[test]
    fn the_compact_restart_hint_keeps_the_glyph() {
        let (text, hit) = draw("v0.4.10", UpdateBadge::Restart("0.4.10"));
        assert_eq!(text, "v0.4.10 ↻");
        assert_eq!(hit, Rect::new(12, 0, 2, 1));
    }

    #[test]
    fn the_compact_hint_still_takes_the_accent() {
        let palette = Palette::catppuccin();
        let mut buf = Buffer::empty(Rect::new(0, 0, 14, 1));
        let hit = put_badge(
            &mut buf,
            Rect::new(0, 0, 14, 1),
            0,
            "v0.4.10",
            UpdateBadge::Update("9.9.9"),
            &palette,
        );
        let arrow = hit.x + 1;
        assert_eq!(buf[(arrow, 0)].symbol(), "↑");
        assert_eq!(buf[(arrow, 0)].fg, palette.accent);
    }

    #[test]
    fn without_an_update_the_version_stands_alone() {
        let (text, hit) = draw("v0.4.9", UpdateBadge::None);
        assert_eq!(text, "v0.4.9");
        assert!(hit.is_empty());
    }

    #[test]
    fn a_header_too_narrow_for_the_version_drops_the_badge() {
        let palette = Palette::catppuccin();
        let mut buf = Buffer::empty(Rect::new(0, 0, 6, 1));
        let hit = put_badge(
            &mut buf,
            Rect::new(0, 0, 6, 1),
            0,
            "v0.4.10",
            UpdateBadge::Update("9.9.9"),
            &palette,
        );
        assert_eq!(row_text(&buf, 6), "");
        assert!(hit.is_empty());
    }

    #[test]
    fn no_hint_when_even_the_compact_one_overflows() {
        // 7 + 2 = 9 cells of compact hint; a 7-cell header leaves none.
        let palette = Palette::catppuccin();
        let mut buf = Buffer::empty(Rect::new(0, 0, 7, 1));
        let hit = put_badge(
            &mut buf,
            Rect::new(0, 0, 7, 1),
            0,
            "v0.4.10",
            UpdateBadge::Update("9.9.9"),
            &palette,
        );
        assert_eq!(row_text(&buf, 7), "v0.4.10");
        assert!(hit.is_empty());
    }

    #[test]
    fn the_compact_width_matches_what_is_drawn() {
        let palette = Palette::catppuccin();
        let badge = UpdateBadge::Update("9.9.9");
        let compact = version_badge_compact_width(badge);
        let mut buf = Buffer::empty(Rect::new(0, 0, compact, 1));
        let hit = put_badge(
            &mut buf,
            Rect::new(0, 0, compact, 1),
            0,
            &running_version(),
            badge,
            &palette,
        );
        // A slot exactly the compact width draws the compact hint whole.
        assert_eq!(hit.width, 2);
        assert!(hit.right() <= compact);
    }
}
