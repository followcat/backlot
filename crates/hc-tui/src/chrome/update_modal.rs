// Derived from herdr src/client/shell/overlays.rs (Apache-2.0): the
// release-notes modal frame, header, footer and centred buttons, with
// the 80x24 size and the vertical stack from src/ui/widgets.rs.
//! The update modal: one card of 80x24 (clamped to the terminal) whose
//! body is the release notes, or the installer status while it runs.

use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::{Modifier, Style};
use ratatui::widgets::{Paragraph, Widget};

use super::dialogs::{button, contrast, row};
use super::palette::Palette;
use super::popup_frame::{PopupChrome, open_centered, overlay_bg};
use super::release_notes::{Stream, stream_lines};
use super::render::{display_width, put_text};
use super::scroll::{ScrollMetrics, render_scrollbar_buffer};
use super::update_status::{Status, Step, done_lines, failed_lines, updating_lines};

/// The herdr release-notes modal size; the card clamps inside it.
pub(crate) const MODAL_SIZE: (u16, u16) = (80, 24);

/// Which card is up.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Phase {
    /// The release notes with the install callout; scrolls.
    Opened,
    /// The installer is running.
    Updating,
    /// Installed and verified; restart is offered.
    Done,
    /// The installer or the version check failed.
    Failed,
    /// The read-only notes of the running version after an upgrade.
    WhatsNew,
}

/// A clickable button; the app maps it to an action.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Button {
    Update,
    Skip,
    Close,
    Cancel,
    Restart,
    Later,
    Retry,
}

/// The body the phase shows.
pub(crate) enum Body<'a> {
    Notes(Stream<'a>),
    Status(Status<'a>),
}

/// Everything one frame of the card is drawn from.
pub(crate) struct ModalView<'a> {
    pub phase: Phase,
    /// The running version (`0.1.11`).
    pub installed: &'a str,
    /// The version shown in the header (`0.1.13`; the running one for
    /// what's new).
    pub latest: &'a str,
    /// Whether `↵ update` is offered (the install dir accepts writes).
    pub writable: bool,
    pub body: Body<'a>,
    /// Rows scrolled from the top of the notes body.
    pub scroll: u16,
}

/// Hit rectangles and scroll bounds from the last frame.
#[derive(Default)]
pub(crate) struct ModalRender {
    pub buttons: Vec<(Rect, Button)>,
    pub track: Option<Rect>,
    /// The largest useful scroll offset for this frame.
    pub max_scroll: u16,
    /// Visible body rows — one page of scrolling.
    pub rows: u16,
}

struct Stack {
    header: Rect,
    content: Rect,
    footer: Option<Rect>,
    actions: Option<Rect>,
}

/// Header (2 rows), then the body, an optional scroll hint and an
/// optional button row, each separated by one blank row.
fn stack(inner: Rect, footer: bool, actions: bool) -> Stack {
    let reserved = 2 * (u16::from(footer) + u16::from(actions));
    let content = Rect::new(
        inner.x,
        inner.y + 3,
        inner.width,
        inner.height.saturating_sub(3 + reserved),
    );
    let mut y = content.bottom();
    let footer = footer.then(|| {
        let rect = Rect::new(inner.x, y + 1, inner.width, 1);
        y = rect.bottom();
        rect
    });
    let actions = actions.then(|| Rect::new(inner.x, y + 1, inner.width, 1));
    Stack {
        header: Rect::new(inner.x, inner.y, inner.width, 2),
        content,
        footer,
        actions,
    }
}

/// Draws the card over `buf` and returns its hit map; `None` when the
#[cfg(test)]
pub(crate) fn render(buf: &mut Buffer, view: &ModalView<'_>, p: &Palette) -> Option<ModalRender> {
    render_with_shadow(buf, view, p, true)
}

pub(crate) fn render_with_shadow(
    buf: &mut Buffer,
    view: &ModalView<'_>,
    p: &Palette,
    shadow: bool,
) -> Option<ModalRender> {
    let geom = open_centered(
        buf,
        buf.area,
        MODAL_SIZE.0.saturating_sub(6),
        MODAL_SIZE.1.saturating_sub(4),
        4,
        4,
        p.accent,
        PopupChrome::Standard,
        p,
        shadow,
    )?;
    let inner = geom.inner;
    let mut out = ModalRender::default();
    if inner.height < 8 || inner.width < 20 {
        return Some(out);
    }
    let scrollable = matches!(view.phase, Phase::Opened | Phase::WhatsNew);
    let stack = stack(inner, scrollable, view.phase != Phase::WhatsNew);
    draw_header(buf, view, &stack.header, p, &mut out);
    draw_body(buf, view, &stack, p, &mut out);
    if let (Some(area), true) = (stack.footer, out.track.is_some()) {
        draw_footer(buf, area, p);
    }
    if let Some(area) = stack.actions {
        draw_actions(buf, view, area, p, &mut out);
    }
    Some(out)
}

fn draw_header(
    buf: &mut Buffer,
    view: &ModalView<'_>,
    header: &Rect,
    p: &Palette,
    out: &mut ModalRender,
) {
    let title = Rect::new(header.x + 1, header.y, header.width.saturating_sub(2), 1);
    let subtitle = Rect::new(title.x, header.y + 1, title.width, 1);
    let base = Style::default()
        .bg(overlay_bg(p))
        .remove_modifier(Modifier::DIM);
    put_text(
        buf,
        title.x,
        title.y,
        title.width,
        &format!("v{}", view.latest),
        base.fg(p.text).add_modifier(Modifier::BOLD),
    );
    let (subtitle_text, right) = match view.phase {
        Phase::Opened => ("update ready".to_owned(), Some(installed(view))),
        Phase::Updating => (
            "updates this client · programs keep running".to_owned(),
            Some(installed(view)),
        ),
        Phase::Done => (
            format!("update installed · restart to use v{}", view.latest),
            Some(format!("v{} running", view.installed)),
        ),
        Phase::Failed => (
            format!("update failed · v{} is unchanged", view.installed),
            Some(installed(view)),
        ),
        Phase::WhatsNew => ("what's new in this release".to_owned(), None),
    };
    put_text(
        buf,
        subtitle.x,
        subtitle.y,
        subtitle.width,
        &subtitle_text,
        base.fg(p.overlay1),
    );
    if let Some(right) = right {
        let width = display_width(&right);
        let x = title.right().saturating_sub(width);
        put_text(buf, x, title.y, width, &right, base.fg(p.overlay0));
    }
    if view.phase == Phase::WhatsNew {
        let label = " esc close ";
        let width = display_width(label);
        let rect = Rect::new(header.right().saturating_sub(width), header.y, width, 1);
        button(
            buf,
            rect,
            label,
            Style::default()
                .fg(contrast(p))
                .bg(p.accent)
                .add_modifier(Modifier::BOLD)
                .remove_modifier(Modifier::DIM),
        );
        out.buttons.push((rect, Button::Close));
    }
}

fn installed(view: &ModalView<'_>) -> String {
    format!("installed v{}", view.installed)
}

fn draw_body(
    buf: &mut Buffer,
    view: &ModalView<'_>,
    stack: &Stack,
    p: &Palette,
    out: &mut ModalRender,
) {
    let content = stack.content;
    let rows = usize::from(content.height);
    out.rows = content.height;
    match &view.body {
        Body::Status(status) => {
            let width = usize::from(content.width);
            let lines = match status.step {
                Step::Done => done_lines(status, width, p),
                Step::Failed => failed_lines(status, width, p),
                Step::Running | Step::Verifying => updating_lines(status, width, p),
            };
            Paragraph::new(lines.into_iter().take(rows).collect::<Vec<_>>()).render(content, buf);
        }
        Body::Notes(stream) => {
            let mut lines = stream_lines(stream, content.width, p);
            let overflow = lines.len() > rows;
            if overflow {
                lines = stream_lines(stream, content.width.saturating_sub(1), p);
            }
            let max_scroll = lines.len().saturating_sub(rows);
            let scroll = usize::from(view.scroll).min(max_scroll);
            let text_area = if overflow {
                Rect::new(content.x, content.y, content.width - 1, content.height)
            } else {
                content
            };
            let visible: Vec<_> = lines.into_iter().skip(scroll).take(rows).collect();
            Paragraph::new(visible).render(text_area, buf);
            out.max_scroll = u16::try_from(max_scroll).unwrap_or(u16::MAX);
            if overflow {
                let track = Rect::new(
                    content.right().saturating_sub(1),
                    content.y,
                    1,
                    content.height,
                );
                render_scrollbar_buffer(
                    buf,
                    ScrollMetrics {
                        offset_from_bottom: max_scroll - scroll,
                        max_offset_from_bottom: max_scroll,
                        viewport_rows: rows,
                    },
                    track,
                    p.overlay0,
                    p.overlay1,
                    "▐",
                );
                out.track = Some(track);
            }
        }
    }
}

fn draw_footer(buf: &mut Buffer, area: Rect, p: &Palette) {
    let base = Style::default()
        .bg(overlay_bg(p))
        .remove_modifier(Modifier::DIM);
    let label = " scroll ";
    put_text(buf, area.x, area.y, area.width, label, base.fg(p.overlay0));
    let x = area.x + display_width(label);
    put_text(
        buf,
        x,
        area.y,
        area.width.saturating_sub(x - area.x),
        "j/k ↑↓ pgup pgdn",
        base.fg(p.text),
    );
}

fn draw_actions(
    buf: &mut Buffer,
    view: &ModalView<'_>,
    area: Rect,
    p: &Palette,
    out: &mut ModalRender,
) {
    let buttons: Vec<(Button, &str, bool)> = match view.phase {
        Phase::Opened if view.writable => vec![
            (Button::Update, " ↵ update ", true),
            (Button::Skip, " s skip ", false),
            (Button::Close, " esc close ", false),
        ],
        Phase::Opened => vec![
            (Button::Skip, " s skip ", false),
            (Button::Close, " esc close ", false),
        ],
        Phase::Updating => vec![(Button::Cancel, " esc cancel ", false)],
        Phase::Done => vec![
            (Button::Restart, " ↵ restart ", true),
            (Button::Later, " esc later ", false),
        ],
        Phase::Failed => vec![
            (Button::Retry, " ↵ retry ", true),
            (Button::Close, " esc close ", false),
        ],
        Phase::WhatsNew => Vec::new(),
    };
    let widths: Vec<u16> = buttons
        .iter()
        .map(|(_, label, _)| display_width(label))
        .collect();
    for (rect, (kind, label, primary)) in row(area, &widths, 2, 0).into_iter().zip(&buttons) {
        let style = if *primary {
            Style::default()
                .fg(contrast(p))
                .bg(p.accent)
                .add_modifier(Modifier::BOLD)
        } else {
            Style::default()
                .fg(p.text)
                .bg(p.surface0)
                .add_modifier(Modifier::BOLD)
        };
        button(buf, rect, label, style);
        out.buttons.push((rect, *kind));
    }
}

#[cfg(test)]
#[path = "update_modal_tests.rs"]
mod tests;

#[cfg(test)]
#[path = "update_modal_source_tests.rs"]
mod source_tests;
