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

#[allow(clippy::too_many_arguments)]
pub(super) fn render_endpoint_row(
    buffer: &mut Buffer,
    rect: Rect,
    marker: &str,
    endpoint: &ClientShellEndpoint,
    highlighted: bool,
    current_machine: bool,
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
    let left_avail = rect.width.saturating_sub(signal_width.saturating_add(1));
    let base_style = Style::default()
        .fg(
            if matches!(endpoint.status, ClientEndpointStatus::Disabled) {
                palette.overlay0
            } else {
                palette.text
            },
        )
        .add_modifier(Modifier::BOLD);
    if current_machine {
        let prefix = format!(" {marker} ");
        let prefix_w = display_width(&prefix).min(left_avail);
        put_text(buffer, rect.x, rect.y, prefix_w, &prefix, base_style);
        let name_x = rect.x.saturating_add(prefix_w);
        let name_avail = left_avail.saturating_sub(prefix_w);
        if name_avail > 0 {
            let accent_bold = Style::default()
                .fg(palette.accent)
                .add_modifier(Modifier::BOLD);
            let name_w = display_width(&endpoint.label);
            if name_w.saturating_add(2) <= name_avail {
                put_text(buffer, name_x, rect.y, name_w, &endpoint.label, accent_bold);
                put_text(
                    buffer,
                    name_x.saturating_add(name_w),
                    rect.y,
                    2,
                    " ✦",
                    Style::default().fg(palette.accent),
                );
            } else {
                let fitted = super::text::truncate_end(&endpoint.label, usize::from(name_avail));
                put_text(buffer, name_x, rect.y, name_avail, &fitted, accent_bold);
            }
        }
    } else {
        put_text(
            buffer,
            rect.x,
            rect.y,
            left_avail,
            &format!(" {marker} {}", endpoint.label),
            base_style,
        );
    }
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
    emphasized: bool,
    palette: &Palette,
) {
    if rect.is_empty() {
        return;
    }
    let dim = Style::default().fg(palette.overlay0);
    let mut text_style = match line.tone {
        MachineTone::Normal => Style::default().fg(palette.text),
        MachineTone::Accent => Style::default().fg(palette.accent),
        MachineTone::Dim => dim,
    };
    if emphasized {
        text_style = text_style.add_modifier(Modifier::BOLD);
    }
    let indent = 3u16;
    let inner = rect.width.saturating_sub(indent);
    if inner == 0 {
        return;
    }
    let x = rect.x.saturating_add(indent);
    let hc = line.hc.as_deref().map(|v| format!("hc {v} "));
    let skill_style = line.skill.map(|mark| {
        let color = match mark {
            crate::chrome::render::MachineSkillMark::Current => palette.green,
            crate::chrome::render::MachineSkillMark::Stale => palette.yellow,
            crate::chrome::render::MachineSkillMark::Missing => palette.peach,
            crate::chrome::render::MachineSkillMark::Unknown => palette.overlay0,
        };
        Style::default().fg(color)
    });
    let draw_stage_suffix = |buf: &mut Buffer, start_x: u16| {
        let Some(ok) = line.stage_ok else {
            return;
        };
        let suffix = if ok { " · ● ok" } else { " · ◌ down" };
        let w = display_width(suffix);
        if start_x.saturating_sub(x) + w > inner {
            return;
        }
        let dot = if ok { "●" } else { "◌" };
        let dot_style = Style::default().fg(if ok { palette.green } else { palette.peach });
        let word = if ok { " ok" } else { " down" };
        put_text(buf, start_x, rect.y, 3, " · ", dim);
        put_text(buf, start_x + 3, rect.y, 1, dot, dot_style);
        put_text(
            buf,
            start_x + 4,
            rect.y,
            display_width(word),
            word,
            if ok { dim } else { dot_style },
        );
    };
    if let (Some(mark), Some(style)) = (line.skill, skill_style)
        && rect.width > 3
    {
        put_text(buffer, rect.x + 1, rect.y, 1, mark.glyph(), style);
    }
    let text_w = display_width(&line.text);
    match hc {
        Some(hc) if display_width(&hc) + text_w <= inner => {
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
            draw_stage_suffix(buffer, x.saturating_add(hc_w).saturating_add(text_w));
        }
        _ => {
            put_text(buffer, x, rect.y, inner, &line.text, text_style);
            draw_stage_suffix(buffer, x.saturating_add(text_w));
        }
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
            stage_ok: None,
            skill: None,
        };
        let mut buf = Buffer::empty(Rect::new(0, 0, 26, 1));
        render_machine_line(&mut buf, Rect::new(0, 0, 26, 1), &line, false, &palette);
        assert_eq!(row_text(&buf, 26), "   hc 0.1.0 backlot 0.2.4");
        let mut buf = Buffer::empty(Rect::new(0, 0, 17, 1));
        render_machine_line(&mut buf, Rect::new(0, 0, 17, 1), &line, false, &palette);
        assert_eq!(row_text(&buf, 17), "   backlot 0.2.4");
    }

    #[test]
    fn stage_ok_suffix_renders_when_space_allows() {
        let palette = Palette::catppuccin();
        let line = MachineLine {
            hc: None,
            text: "backlot 0.4.12".into(),
            tone: MachineTone::Normal,
            stage_ok: Some(true),
            skill: None,
        };
        let mut buf = Buffer::empty(Rect::new(0, 0, 25, 1));
        render_machine_line(&mut buf, Rect::new(0, 0, 25, 1), &line, false, &palette);
        assert_eq!(row_text(&buf, 25), "   backlot 0.4.12 · ● ok");
        assert!(!buf[(3, 0)].modifier.contains(Modifier::BOLD));

        let mut ebuf = Buffer::empty(Rect::new(0, 0, 25, 1));
        render_machine_line(&mut ebuf, Rect::new(0, 0, 25, 1), &line, true, &palette);
        assert!(ebuf[(3, 0)].modifier.contains(Modifier::BOLD));
    }

    #[test]
    fn the_skill_marker_has_its_state_colour() {
        let palette = Palette::catppuccin();
        let line = MachineLine {
            hc: None,
            text: "backlot 0.4.12".into(),
            tone: MachineTone::Normal,
            stage_ok: Some(true),
            skill: Some(crate::chrome::render::MachineSkillMark::Missing),
        };
        let mut buf = Buffer::empty(Rect::new(0, 0, 26, 1));
        render_machine_line(&mut buf, Rect::new(0, 0, 26, 1), &line, false, &palette);
        assert_eq!(buf[(1, 0)].symbol(), "!");
        assert_eq!(buf[(1, 0)].fg, palette.peach);
    }
}
