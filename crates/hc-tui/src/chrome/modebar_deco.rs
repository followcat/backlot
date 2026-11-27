//! Babylon Deco plaques (`╡ FOCUS: LOT · <studio> ╞`) on `stage_well` for the modebar (S4 §5).

use ratatui::{
    buffer::Buffer,
    layout::Rect,
    style::{Color, Modifier, Style},
};
use unicode_width::UnicodeWidthStr;

use super::ClientShellMode;
use crate::app::copy_mode::CopyModeState;
use crate::chrome::palette::Palette;
use crate::chrome::render::{put_right_text, put_segment};
use crate::chrome::status::panel_contrast_fg;
use crate::config::LiveKeybindConfig;
use crate::copy_mode::CopySearchDirection;

fn prefix_rhs(bindings: &crate::config::ActionKeybinds) -> String {
    bindings
        .prefix_rhs_label()
        .unwrap_or_else(|| "unset".to_owned())
}

fn plaque_fg(palette: &Palette) -> Color {
    let stage = palette.elevation().stage_well;
    if stage == Color::Reset {
        panel_contrast_fg(palette)
    } else {
        stage
    }
}

/// Format a `focus.rs` breadcrumb body into Babylon modebar casing:
/// e.g. `"lot › acme-shop"` -> `"LOT · acme-shop"`, `"tree"` -> `"TREE"`.
pub(crate) fn format_focus_body(body: &str) -> String {
    let mut parts = body.split(" › ");
    let Some(head) = parts.next() else {
        return String::new();
    };
    let head_upper = head.to_ascii_uppercase();
    let rest: Vec<&str> = parts.collect();
    if rest.is_empty() {
        head_upper
    } else {
        format!("{head_upper} · {}", rest.join(" · "))
    }
}

#[allow(clippy::too_many_arguments)]
pub(super) fn render_deco_mode_bar(
    buffer: &mut Buffer,
    bar: Rect,
    mode: ClientShellMode,
    keyboard_locked: bool,
    copy_mode: Option<&CopyModeState>,
    endpoint_error: Option<&str>,
    update_available: bool,
    restart_ready: bool,
    stage_log: bool,
    focus_breadcrumb: Option<&str>,
    keybinds: &LiveKeybindConfig,
    palette: &Palette,
) -> Option<Rect> {
    let elev = palette.elevation();
    let bar_bg = elev.stage_well;
    let base = Style::default().fg(elev.muted_surface).bg(bar_bg);
    let div = Style::default().fg(elev.gilt).bg(bar_bg);
    let key = Style::default()
        .fg(palette.accent)
        .bg(bar_bg)
        .add_modifier(Modifier::BOLD);

    let plaque_bg = if keyboard_locked || endpoint_error.is_some() {
        palette.red
    } else if mode == ClientShellMode::Resize {
        palette.mauve
    } else {
        palette.accent
    };
    let plaque_style = Style::default()
        .fg(plaque_fg(palette))
        .bg(plaque_bg)
        .add_modifier(Modifier::BOLD);

    for x in bar.x..bar.x + bar.width {
        buffer[(x, bar.y)].set_symbol(" ").set_style(base);
    }

    let sep = (" · ".to_owned(), div);
    let prefix = keybinds.primary_prefix_label();
    let mut segments = Vec::<(String, Style)>::new();

    if keyboard_locked {
        segments.extend([
            ("╡ LOCKED ╞".to_owned(), plaque_style),
            ("  ".to_owned(), base),
            ("ctrl+g".to_owned(), key),
            (" unlock".to_owned(), base),
        ]);
    } else if let Some(error) = endpoint_error {
        segments.extend([
            ("╡ ERROR ╞".to_owned(), plaque_style),
            (format!("  {error}"), base),
        ]);
    } else {
        match mode {
            ClientShellMode::Prefix => {
                segments.extend([
                    ("╡ PREFIX ╞".to_owned(), plaque_style),
                    ("  ".to_owned(), base),
                    ("esc".to_owned(), key),
                    (" cancel".to_owned(), base),
                    sep.clone(),
                    (prefix, key),
                    (" send prefix".to_owned(), base),
                    sep.clone(),
                    (prefix_rhs(&keybinds.keybinds.workspace_picker), key),
                    (" workspace nav".to_owned(), base),
                    sep.clone(),
                    (prefix_rhs(&keybinds.keybinds.help), key),
                    (" keybinds".to_owned(), base),
                ]);
            }
            ClientShellMode::Navigate => {
                segments.extend([
                    ("╡ NAVIGATE ╞".to_owned(), plaque_style),
                    ("  ".to_owned(), base),
                    ("esc back".to_owned(), base),
                    sep.clone(),
                    ("↑/↓".to_owned(), key),
                    (" workspace".to_owned(), base),
                    sep.clone(),
                    ("tab".to_owned(), key),
                    (" pane".to_owned(), base),
                    sep.clone(),
                    (prefix_rhs(&keybinds.keybinds.help), key),
                    (" keybinds".to_owned(), base),
                ]);
            }
            ClientShellMode::Resize => {
                segments.extend([
                    ("╡ RESIZE ╞".to_owned(), plaque_style),
                    ("  ".to_owned(), base),
                    ("h/l".to_owned(), key),
                    (" width".to_owned(), base),
                    sep.clone(),
                    ("j/k".to_owned(), key),
                    (" height".to_owned(), base),
                    sep.clone(),
                    ("esc".to_owned(), key),
                    (" done".to_owned(), base),
                ]);
            }
            ClientShellMode::Copy => {
                let copy_mode = copy_mode?;
                if let Some(prompt) = copy_mode.search_prompt.as_ref() {
                    let marker = match prompt.direction {
                        CopySearchDirection::Forward => "/",
                        CopySearchDirection::Backward => "?",
                    };
                    buffer.set_stringn(
                        bar.x,
                        bar.y,
                        "╡ COPY ╞",
                        usize::from(bar.width),
                        plaque_style,
                    );
                    let prefix_w = 10.min(bar.width);
                    if bar.width >= 10 {
                        buffer.set_string(bar.x + 9, bar.y, marker, key);
                    }
                    let footer = "  enter search · esc cancel";
                    let footer_width = if bar.width >= 52 {
                        UnicodeWidthStr::width(footer) as u16
                    } else {
                        0
                    };
                    let field = Rect::new(
                        bar.x + prefix_w,
                        bar.y,
                        bar.width.saturating_sub(prefix_w + footer_width),
                        1,
                    );
                    if let Some(cursor) = super::super::text_editor::render(
                        buffer,
                        field,
                        &prompt.query,
                        Style::default().fg(palette.text).bg(bar_bg),
                    ) {
                        buffer[(cursor.x, cursor.y)]
                            .set_style(Style::default().fg(bar_bg).bg(palette.text));
                    }
                    if footer_width > 0 {
                        buffer.set_string(bar.right() - footer_width, bar.y, footer, base);
                    }
                    return Some(bar);
                }
                let select = if copy_mode.selection.is_some() {
                    "selecting"
                } else {
                    "select"
                };
                let match_status = copy_mode
                    .search_current_global
                    .map(|current| format!(" {}/{}", current + 1, copy_mode.search_total))
                    .or_else(|| (!copy_mode.search_query.is_empty()).then(|| " 0/0".to_owned()))
                    .unwrap_or_default();
                let (exit_keys, exit_label) =
                    if copy_mode.search_query.is_empty() && copy_mode.selection.is_none() {
                        ("q/esc", " exit")
                    } else {
                        ("esc", " clear · q exit")
                    };
                segments.extend([
                    ("╡ COPY ╞".to_owned(), plaque_style),
                    ("  ".to_owned(), base),
                    ("h/j/k/l w/b/e { }".to_owned(), key),
                    (" move".to_owned(), base),
                    sep.clone(),
                    ("/ ?".to_owned(), key),
                    (" search".to_owned(), base),
                    sep.clone(),
                    ("n/N".to_owned(), key),
                    (format!(" repeat{match_status}"), base),
                    sep.clone(),
                    ("v/space".to_owned(), key),
                    (format!(" {select}"), base),
                    sep.clone(),
                    ("y/enter".to_owned(), key),
                    (" copy".to_owned(), base),
                    sep,
                    (exit_keys.to_owned(), key),
                    (exit_label.to_owned(), base),
                ]);
            }
            ClientShellMode::Terminal => {
                if stage_log {
                    segments.extend([
                        ("╡ STAGE LOG ╞".to_owned(), plaque_style),
                        ("  stage log — read only".to_owned(), base),
                    ]);
                } else if let Some(crumb) = focus_breadcrumb {
                    let body = crumb.strip_prefix("FOCUS:").unwrap_or(crumb).trim_start();
                    let label = format_focus_body(body);
                    segments.extend([
                        (format!("╡ FOCUS: {label} ╞"), plaque_style),
                        ("  ".to_owned(), base),
                        ("Alt+L".to_owned(), key),
                        (" lot".to_owned(), base),
                        sep.clone(),
                        ("Alt+Y".to_owned(), key),
                        (" call sheet".to_owned(), base),
                        sep.clone(),
                        ("Alt+←→".to_owned(), key),
                        (" studio".to_owned(), base),
                        sep,
                        ("C-b t/a".to_owned(), key),
                        (" sidebar".to_owned(), base),
                    ]);
                }
            }
        }
    }

    let mut x = bar.x;
    for (text, style) in segments {
        if x >= bar.right() {
            break;
        }
        put_segment(buffer, x, bar.y, bar.right(), &text, style);
        x = x.saturating_add(u16::try_from(UnicodeWidthStr::width(text.as_str())).unwrap_or(0));
    }

    let update_status = if restart_ready {
        Some((" restart ready", palette.green))
    } else if update_available {
        Some((" update ready", palette.accent))
    } else {
        None
    };
    if let Some((text, color)) = update_status.filter(|_| mode == ClientShellMode::Navigate) {
        let width = u16::try_from(UnicodeWidthStr::width(text))
            .unwrap_or(u16::MAX)
            .min(bar.width);
        let area = Rect::new(bar.right().saturating_sub(width), bar.y, width, 1);
        buffer.set_style(area, Style::default().bg(bar_bg));
        put_right_text(
            buffer,
            area,
            bar.y,
            text,
            Style::default()
                .fg(color)
                .bg(bar_bg)
                .add_modifier(Modifier::BOLD),
        );
    }
    Some(bar)
}
