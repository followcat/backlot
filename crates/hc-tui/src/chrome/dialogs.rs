// Derived from herdr src/client/shell/overlays.rs (Apache-2.0).
use super::{
    palette::Palette,
    render::{display_width, put_text},
    text_editor::{self, TextEditor},
};
use ratatui::{
    buffer::Buffer,
    layout::Rect,
    style::{Modifier, Style},
};
#[derive(Default)]
pub(crate) struct DialogRender {
    pub area: Rect,
    pub primary: Rect,
    pub clear: Rect,
    pub cancel: Rect,
    pub cursor: Option<super::help::CursorState>,
}
pub(crate) struct Rename {
    pub title: &'static str,
    pub input: TextEditor,
}
pub(crate) struct Confirm {
    pub title: String,
    pub detail: String,
}
pub(crate) fn button(b: &mut Buffer, r: Rect, t: &str, s: Style) {
    b.set_style(r, s);
    let w = display_width(t).min(r.width);
    put_text(b, r.x + (r.width - w) / 2, r.y, w, t, s)
}

pub(crate) fn row(i: Rect, ws: &[u16], gap: u16, off: u16) -> Vec<Rect> {
    let total = ws.iter().sum::<u16>() + gap * (ws.len().saturating_sub(1) as u16);
    let mut x = i.x + i.width.saturating_sub(total) / 2;
    ws.iter()
        .map(|w| {
            let r = Rect::new(
                x,
                i.y + off.min(i.height.saturating_sub(1)),
                (*w).min(i.width.saturating_sub(x - i.x)),
                1,
            );
            x += *w + gap;
            r
        })
        .collect()
}

pub(crate) fn contrast(p: &Palette) -> ratatui::style::Color {
    if p.panel_bg == ratatui::style::Color::Reset {
        p.surface_dim
    } else {
        super::popup_frame::contrast(p)
    }
}

pub(crate) fn render_rename_styled(
    b: &mut Buffer,
    v: &Rename,
    p: &Palette,
    shadow: bool,
    deco: bool,
) -> Option<DialogRender> {
    let chrome = if deco {
        super::popup_frame::PopupChrome::Deco
    } else {
        super::popup_frame::PopupChrome::Standard
    };
    let content_h = if deco { 3 } else { 5 };
    let geom = super::popup_frame::open_centered(
        b, b.area, 54, content_h, 4, 3, p.accent, chrome, p, shadow,
    )?;
    let q = geom.outer;
    let i = geom.inner;
    let bg = super::popup_frame::overlay_bg(p);
    let field_bg = p.elevation().raised_on_overlay;
    if deco {
        super::popup_deco::draw_deco_title(b, geom.border, v.title, p.accent, false, p);
    } else {
        put_text(
            b,
            i.x,
            i.y,
            i.width,
            v.title,
            Style::default()
                .fg(p.text)
                .bg(bg)
                .add_modifier(Modifier::BOLD),
        );
    }
    let input_y = if deco { i.y } else { i.y + 2 };
    let input = Rect::new(i.x, input_y, i.width, 1);
    b.set_style(input, Style::default().fg(p.text).bg(field_bg));
    let cursor = text_editor::render(
        b,
        Rect::new(input.x + 1, input.y, input.width.saturating_sub(1), 1),
        &v.input,
        Style::default().fg(p.text).bg(field_bg),
    );
    let rs = if deco {
        row(i, &[10, 10, 12], 2, 2)
    } else {
        row(i, &[8, 10, 12], 2, 3)
    };
    let [save, clear, cancel] = rs.as_slice() else {
        return None;
    };
    let save_label = super::popup_deco::primary_label(" ↵ save ", deco);
    button(
        b,
        *save,
        &save_label,
        Style::default()
            .fg(contrast(p))
            .bg(p.accent)
            .add_modifier(Modifier::BOLD),
    );
    let n = Style::default()
        .fg(p.text)
        .bg(field_bg)
        .add_modifier(Modifier::BOLD);
    button(b, *clear, " ^c clear ", n);
    button(b, *cancel, " esc cancel ", n);
    Some(DialogRender {
        area: q,
        primary: *save,
        clear: *clear,
        cancel: *cancel,
        cursor,
    })
}

pub(crate) fn render_confirm_styled(
    b: &mut Buffer,
    c: &Confirm,
    p: &Palette,
    shadow: bool,
    deco: bool,
) -> Option<DialogRender> {
    let chrome = if deco {
        super::popup_frame::PopupChrome::Deco
    } else {
        super::popup_frame::PopupChrome::Standard
    };
    let content_h = if deco { 3 } else { 4 };
    let geom = super::popup_frame::open_centered(
        b, b.area, 62, content_h, 4, 3, p.red, chrome, p, shadow,
    )?;
    let q = geom.outer;
    let i = geom.inner;
    let bg = super::popup_frame::overlay_bg(p);
    let field_bg = p.elevation().raised_on_overlay;
    if deco {
        super::popup_deco::draw_deco_title(b, geom.border, &c.title, p.red, true, p);
    } else {
        put_text(
            b,
            i.x,
            i.y,
            i.width,
            &format!(" {}", c.title),
            Style::default()
                .fg(p.red)
                .bg(bg)
                .add_modifier(Modifier::BOLD),
        );
    }
    let detail_y = if deco { i.y } else { i.y + 1 };
    put_text(
        b,
        i.x,
        detail_y,
        i.width,
        &format!(" {}", c.detail),
        Style::default().fg(p.text).bg(bg),
    );
    let rs = if deco {
        row(i, &[13, 12], 2, 2)
    } else {
        row(i, &[13, 12], 2, 3)
    };
    let [ok, cancel] = rs.as_slice() else {
        return None;
    };
    let ok_label = super::popup_deco::primary_label(" ↵ confirm ", deco);
    button(
        b,
        *ok,
        &ok_label,
        Style::default()
            .fg(contrast(p))
            .bg(p.red)
            .add_modifier(Modifier::BOLD),
    );
    button(
        b,
        *cancel,
        " esc cancel ",
        Style::default()
            .fg(p.text)
            .bg(field_bg)
            .add_modifier(Modifier::BOLD),
    );
    Some(DialogRender {
        area: q,
        primary: *ok,
        clear: Rect::default(),
        cancel: *cancel,
        cursor: None,
    })
}
