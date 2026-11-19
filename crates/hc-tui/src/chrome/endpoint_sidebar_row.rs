//! One row in the expanded endpoint list: the endpoint label plus its
//! status or auth signal at the right.

use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::{Modifier, Style};

use super::palette::Palette;
use super::render::{
    MachineDiagnostics, MachineLine, MachineTone, ShellRenderState, display_width, put_right_text,
    put_text,
};
use super::snapshot::{ClientEndpointStatus, ClientShellEndpoint};

pub(super) fn active_endpoint_label<'a>(state: &'a ShellRenderState<'_>) -> &'a str {
    state
        .endpoints
        .iter()
        .find(|endpoint| &endpoint.endpoint_id == state.active_endpoint_id)
        .map_or("Local", |endpoint| endpoint.label.as_str())
}

pub(super) fn render_endpoint_row(
    buffer: &mut Buffer,
    rect: Rect,
    marker: &str,
    endpoint: &ClientShellEndpoint,
    highlighted: bool,
    auth: &MachineDiagnostics,
    palette: &Palette,
) -> Rect {
    if highlighted {
        buffer.set_style(rect, Style::default().bg(palette.active_row_bg));
    }
    let (glyph, state, color) =
        super::endpoints::endpoint_status_presentation(endpoint.status, palette);
    let state = if endpoint.status == ClientEndpointStatus::Online {
        ""
    } else {
        state
    };
    let signal = if auth.required_for(endpoint) {
        "! auth".to_owned()
    } else if endpoint.status == ClientEndpointStatus::Attention {
        "! error".to_owned()
    } else if endpoint.endpoint_id.is_local() {
        String::new()
    } else if state.is_empty() {
        glyph.to_owned()
    } else {
        format!("{glyph} {state}")
    };
    let signal_width = display_width(&signal).min(rect.width);
    put_text(
        buffer,
        rect.x,
        rect.y,
        rect.width.saturating_sub(signal_width.saturating_add(1)),
        &format!(" {marker} {}", endpoint.label),
        Style::default()
            .fg(
                if matches!(endpoint.status, ClientEndpointStatus::Disabled) {
                    palette.overlay0
                } else {
                    palette.text
                },
            )
            .add_modifier(Modifier::BOLD),
    );
    put_right_text(
        buffer,
        rect,
        rect.y,
        &signal,
        auth.badge_style(endpoint, palette, Style::default().fg(color)),
    );
    Rect::new(
        rect.right().saturating_sub(signal_width),
        rect.y,
        signal_width,
        1,
    )
}

/// U6c: the machine row's version sub-line — `hc <v>` dim ahead of
/// the backlot text when the pair fits; at narrow widths the `hc`
/// segment drops first so the backlot fact stays visible. `put_text`
/// clips the rest, so nothing spills past the sidebar edge.
pub(super) fn render_machine_line(
    buffer: &mut Buffer,
    rect: Rect,
    line: &MachineLine,
    palette: &Palette,
) {
    if rect.is_empty() {
        return;
    }
    let dim = Style::default().fg(palette.overlay0);
    let text_style = match line.tone {
        MachineTone::Normal => Style::default().fg(palette.text),
        MachineTone::Accent => Style::default().fg(palette.accent),
        MachineTone::Dim => dim,
    };
    let indent = 3u16;
    let inner = rect.width.saturating_sub(indent);
    if inner == 0 {
        return;
    }
    let x = rect.x.saturating_add(indent);
    let hc = line.hc.as_deref().map(|v| format!("hc {v} "));
    match hc {
        Some(hc) if display_width(&hc) + display_width(&line.text) <= inner => {
            let hc_w = display_width(&hc);
            put_text(buffer, x, rect.y, hc_w, &hc, dim);
            put_text(
                buffer,
                x.saturating_add(hc_w),
                rect.y,
                inner.saturating_sub(hc_w),
                &line.text,
                text_style,
            );
        }
        _ => put_text(buffer, x, rect.y, inner, &line.text, text_style),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::chrome::render::{MachineLine, MachineTone};

    fn row_text(buf: &Buffer, width: u16) -> String {
        (0..width)
            .map(|x| buf[(x, 0)].symbol())
            .collect::<String>()
            .trim_end()
            .to_owned()
    }

    #[test]
    fn a_narrow_row_drops_the_hc_segment_first() {
        let palette = Palette::catppuccin();
        let line = MachineLine {
            hc: Some("0.1.0".into()),
            text: "backlot 0.2.4".into(),
            tone: MachineTone::Normal,
        };
        let mut buf = Buffer::empty(Rect::new(0, 0, 26, 1));
        render_machine_line(&mut buf, Rect::new(0, 0, 26, 1), &line, &palette);
        assert_eq!(row_text(&buf, 26), "   hc 0.1.0 backlot 0.2.4");
        let mut buf = Buffer::empty(Rect::new(0, 0, 17, 1));
        render_machine_line(&mut buf, Rect::new(0, 0, 17, 1), &line, &palette);
        assert_eq!(row_text(&buf, 17), "   backlot 0.2.4");
    }
}
