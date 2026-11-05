// Derived from herdr src/ui/status.rs (Apache-2.0).
use super::{palette::Palette, status::panel_contrast_fg};
use ratatui::{
    buffer::Buffer,
    layout::Rect,
    style::{Modifier, Style},
    text::Span,
    widgets::{Clear, Paragraph, Widget},
};

pub(crate) fn render(
    buffer: &mut Buffer,
    area: Rect,
    message: &str,
    palette: &Palette,
    mut covered: impl FnMut(Rect),
) {
    let style = Style::default()
        .fg(panel_contrast_fg(palette))
        .bg(palette.yellow)
        .add_modifier(Modifier::BOLD);
    for (row, line) in message
        .lines()
        .filter(|line| !line.trim().is_empty())
        .take(area.height as usize)
        .enumerate()
    {
        let text = format!(" {line} ");
        let width = (text.len() as u16).min(area.width);
        let rect = Rect::new(
            area.x + area.width.saturating_sub(width),
            area.y + row as u16,
            width,
            1,
        );
        covered(rect);
        Clear.render(rect, buffer);
        Paragraph::new(Span::styled(text, style)).render(rect, buffer);
    }
}
