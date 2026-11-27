//! Babylon Deco sidebar header (` ✦ MACHINES      v<version>`) and footer `+` accent (S4 §7).

use ratatui::{
    buffer::Buffer,
    layout::Rect,
    style::{Modifier, Style},
};

use super::badge::{UpdateBadge, put_version_badge, put_version_badge_styled};
use super::endpoint_sidebar_row::active_endpoint_label;
use super::palette::Palette;
use super::render::{ShellHitMap, ShellRenderState, display_width, put_right_text, put_text};

pub(super) fn edge_style(
    palette: &Palette,
    default_edge: ratatui::style::Color,
    bg: ratatui::style::Color,
    studio_chrome: bool,
) -> Style {
    let elev = palette.elevation();
    if studio_chrome {
        elev.gilt_style(bg)
    } else {
        Style::default().fg(default_edge).bg(bg)
    }
}

pub(super) fn render_machines_header(
    buffer: &mut Buffer,
    workspace_area: Rect,
    tree_focused: bool,
    studio_chrome: bool,
    update_badge: UpdateBadge<'_>,
    palette: &Palette,
) -> Rect {
    if workspace_area.is_empty() {
        return Rect::default();
    }
    if !studio_chrome {
        let header_fg = if tree_focused {
            palette.accent
        } else {
            palette.overlay0
        };
        let header_title = if tree_focused {
            if workspace_area.width >= 20 {
                " ▸ machines [C-b t]"
            } else {
                " ▸ machines"
            }
        } else {
            " machines"
        };
        put_text(
            buffer,
            workspace_area.x,
            workspace_area.y,
            workspace_area.width,
            header_title,
            Style::default().fg(header_fg).add_modifier(Modifier::BOLD),
        );
        if tree_focused {
            return Rect::default();
        }
        let badge_row = Rect::new(
            workspace_area.x,
            workspace_area.y,
            workspace_area.width.saturating_sub(1),
            1,
        );
        return put_version_badge(
            buffer,
            badge_row,
            workspace_area.x.saturating_add(10),
            update_badge,
            palette,
        );
    }
    let elev = palette.elevation();
    let bg = elev.stage_well;
    let header_row = Rect::new(workspace_area.x, workspace_area.y, workspace_area.width, 1);
    buffer.set_style(header_row, Style::default().bg(bg));

    let glyph = if tree_focused { "▸" } else { "✦" };
    let label_fg = if tree_focused {
        palette.accent
    } else {
        palette.subtext0
    };
    put_text(
        buffer,
        workspace_area.x,
        workspace_area.y,
        1,
        " ",
        Style::default().fg(palette.overlay0).bg(bg),
    );
    if workspace_area.width >= 2 {
        put_text(
            buffer,
            workspace_area.x + 1,
            workspace_area.y,
            1,
            glyph,
            Style::default()
                .fg(palette.accent)
                .bg(bg)
                .add_modifier(Modifier::BOLD),
        );
    }
    let title = if tree_focused && workspace_area.width >= 20 {
        " MACHINES [C-b t]"
    } else {
        " MACHINES"
    };
    if workspace_area.width > 2 {
        put_text(
            buffer,
            workspace_area.x + 2,
            workspace_area.y,
            workspace_area.width - 2,
            title,
            Style::default()
                .fg(label_fg)
                .bg(bg)
                .add_modifier(Modifier::BOLD),
        );
    }
    if tree_focused {
        return Rect::default();
    }
    let badge_row = Rect::new(
        workspace_area.x,
        workspace_area.y,
        workspace_area.width.saturating_sub(1),
        1,
    );
    put_version_badge_styled(
        buffer,
        badge_row,
        workspace_area.x.saturating_add(12),
        update_badge,
        palette,
        elev.muted_surface,
        Some(bg),
    )
}

pub(super) fn render_footer_action(
    buffer: &mut Buffer,
    workspace_area: Rect,
    footer_y: u16,
    state: &ShellRenderState<'_>,
    palette: &Palette,
    hits: &mut ShellHitMap,
) {
    let rest = format!(" new · {}", active_endpoint_label(state));
    let full_w = 1u16.saturating_add(display_width(&rest));
    hits.new_workspace = Rect::new(
        workspace_area.x,
        footer_y,
        full_w.min(workspace_area.width),
        u16::from(workspace_area.height > 0),
    );
    put_text(
        buffer,
        workspace_area.x,
        footer_y,
        1.min(workspace_area.width),
        "+",
        Style::default().fg(palette.accent),
    );
    if workspace_area.width > 1 {
        put_text(
            buffer,
            workspace_area.x + 1,
            footer_y,
            workspace_area.width - 1,
            &rest,
            Style::default().fg(palette.overlay0),
        );
    }
    let dot = state.update_badge.accent_style(palette);
    let width = if dot.is_some() { 8 } else { 6 };
    let width = width.min(workspace_area.width);
    hits.global_launcher = Rect::new(
        workspace_area.right().saturating_sub(width),
        footer_y,
        width,
        1,
    );
    put_right_text(
        buffer,
        workspace_area,
        footer_y,
        "menu",
        Style::default().fg(palette.overlay0),
    );
    if let Some(style) = dot.filter(|_| workspace_area.width >= 6) {
        put_text(
            buffer,
            workspace_area.right().saturating_sub(6),
            footer_y,
            2,
            "● ",
            style,
        );
    }
}
