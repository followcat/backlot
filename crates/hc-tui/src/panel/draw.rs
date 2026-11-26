//! The panel's renderer (U1): a `studios <n>` title, then one
//! bordered block per studio — `╭ <id> … !<n> ╮` over one-line
//! people in `├──┤`-divided groups — stacking every other block to a
//! `╶ <id> …` line when they do not all fit, scrolling under the
//! title after that. A dim shortcuts row sits last when the panel is
//! at least 8 rows tall. Pure drawing: it reads `Panel` and the
//! summary, writes cells, returns `PanelRender` (hits + whether a
//! working mark reached the screen).

use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::{Modifier, Style};

use super::config::StudioKeys;
use super::data::{MachineSummary, spans_machines, studio_count, studio_label};
use super::draw_blocks::{
    draw_input, draw_outward_card, draw_owed, draw_person, draw_reopen, draw_talk, draw_top,
    draw_usage, frame_style,
};
use super::draw_cards::{draw_stacked, draw_strip, draw_strip_role, draw_strip_summary};
use super::draw_compact::{draw_calm_summary, draw_compact_studio};
use super::draw_tasks::{draw_closed_fold, draw_drawer, draw_fold, draw_task};
use super::rows::{Row, Target};
use super::{Panel, PanelMode, rows};
use crate::chrome::palette::Palette;
use crate::chrome::render::put_text;
use crate::chrome::scroll::{ScrollMetrics, list_scroll_metrics, render_list_scrollbar};
use crate::chrome::text::{display_width, truncate_end};
use crate::studio::StudioKey;
use crate::studio::glyphs::{self, Person};

/// What the mouse needs plus the animation fact: a working mark
/// reached the frame (the loop arms the ~120 ms tick only then).
#[derive(Debug, Default)]
pub(crate) struct PanelRender {
    /// The panel's whole rect (column or full screen).
    pub area: Rect,
    /// The title row.
    pub title: Rect,
    /// The scrolled body under the title (before the scrollbar column).
    pub body: Rect,
    /// Hit rows under the title, with their targets.
    pub rows: Vec<(Rect, Target)>,
    /// The scrollbar track (empty when no overflow).
    pub scrollbar: Rect,
    /// The applied scroll metrics — drag math and max scroll.
    pub metrics: Option<ScrollMetrics>,
    /// Rows visible per page (PageUp/PageDown step).
    pub viewport_rows: usize,
    /// A working mark was drawn this frame.
    pub working_drawn: bool,
    /// The `›` line's editor cursor while its studio composes —
    /// `None` while every input row rests.
    pub cursor: Option<crate::chrome::help::CursorState>,
}

/// Render the panel into `area`. `body` rows under the title scroll;
/// the rightmost column carries the scrollbar on overflow; the last
/// row is the dim shortcuts line (dropped under 8 rows). `local` is
/// this core's display name — it labels local studios and local
/// stage lines when studios span machines. `pending` is the S3
/// ActionTracker: a row whose target mark is in flight shows the
/// dim `⠋ <label>` at its right edge (§3.1). `overlay` is whether
/// the dock draws over the pane surface at this width; the footer's
/// `esc`/`esc back`/`esc close` label reads it together with
/// `pane_refuses` — the same two facts the Esc key reads
/// (`App::panel_esc_closes`), so label and key cannot disagree.
#[allow(clippy::too_many_arguments)]
pub(crate) fn render(
    buf: &mut Buffer,
    area: Rect,
    machines: &[MachineSummary],
    stage_running: impl Fn(Option<&str>) -> bool,
    reachable: impl Fn(Option<&str>) -> bool,
    panel: &Panel,
    mode: PanelMode,
    current: Option<&StudioKey>,
    // The studio the `▸` marker may paint on — `current` when a
    // studio workspace is focused, `None` over `backlot`/`stage` or
    // a plain workspace (R3-M6: a remembered studio is not active).
    mark: Option<&StudioKey>,
    // The `current` studio's cast size (roles + shells) — the
    // footer's `↑↓ role` hint reads it (wide-01).
    _cast_entries: usize,
    local: Option<&str>,
    keys: &StudioKeys,
    _sheet: Option<String>,
    overlay: bool,
    pane_refuses: bool,
    active_surface: bool,
    pending: &crate::action::ActionTracker,
    palette: &Palette,
    now_ms: u64,
) -> PanelRender {
    // Reset every cell first: `set_style` alone leaves the symbols the
    // frame underneath drew — a full-screen panel must not show pane
    // borders or titles through its blank cells.
    for y in area.y..area.y + area.height {
        for x in area.x..area.x + area.width {
            if let Some(cell) = buf.cell_mut((x, y)) {
                cell.reset();
            }
        }
    }
    let elev = palette.elevation();
    buf.set_style(area, Style::default().bg(elev.surface));
    let mut hits = PanelRender {
        area,
        ..PanelRender::default()
    };
    if area.width == 0 || area.height == 0 {
        return hits;
    }
    if area.width > 1 {
        let seam = Style::default().fg(elev.border_muted).bg(elev.surface);
        for y in area.y..area.bottom() {
            if let Some(cell) = buf.cell_mut((area.x, y)) {
                cell.set_symbol("│");
                cell.set_style(seam);
            }
        }
    }
    let inner = if area.width > 1 {
        Rect::new(area.x + 1, area.y, area.width - 1, area.height)
    } else {
        area
    };

    let lot_focused = panel.focused && active_surface;
    // `lot · <n>` — accent while the panel owns the keys; the call
    // sheet's key sits at the row's right end when it fits.
    let title = Rect::new(inner.x, inner.y, inner.width, 1);
    let title_fg = if lot_focused {
        palette.accent
    } else {
        palette.text
    };
    let studios = studio_count(machines);
    let title_text = match mode {
        PanelMode::Collapsed => format!(" lot {studios}"),
        _ => format!(" lot · {studios}"),
    };
    put_text(
        buf,
        title.x,
        title.y,
        title.width,
        &title_text,
        Style::default().fg(title_fg).add_modifier(Modifier::BOLD),
    );
    if let Some(hint) = super::draw_hints::header_hint(
        panel.focused,
        overlay,
        pane_refuses,
        mode,
        keys,
        area.width,
        title
            .width
            .saturating_sub(display_width(&title_text) as u16 + 1) as usize,
    ) {
        let hint_width = display_width(&hint) as u16;
        put_text(
            buf,
            title.x + title.width - hint_width,
            title.y,
            hint_width,
            &hint,
            Style::default().fg(elev.muted_surface),
        );
    }
    hits.title = title;

    // Footer hint isolation: when unfocused, keep bottom row blank `elev.surface`;
    // when focused, draw a `border_muted` hairline above up to 3 verbful hints.
    let shortcuts = area.height >= 8;
    let footer_rows = if shortcuts {
        if panel.focused { 2 } else { 1 }
    } else {
        0
    };
    if shortcuts && panel.focused {
        let sep_y = inner.y + inner.height - 2;
        put_text(
            buf,
            inner.x,
            sep_y,
            inner.width,
            &"─".repeat(usize::from(inner.width)),
            Style::default().fg(elev.border_muted).bg(elev.surface),
        );
        let pairs = super::draw_hints::focused_hint_pairs(
            panel.selected.as_ref(),
            panel.editing,
            panel.finish_confirm.is_some(),
            machines,
            overlay,
            pane_refuses,
            usize::from(inner.width.saturating_sub(1)),
        );
        crate::chrome::render::put_hint_bar(
            buf,
            inner.x + 1,
            inner.y + inner.height - 1,
            inner.width.saturating_sub(1),
            &pairs,
            palette,
        );
    }

    // Body rows scroll under the title; the scrollbar keeps the last
    // column of the body.
    let body = Rect::new(
        inner.x,
        inner.y + 1,
        inner.width,
        inner.height.saturating_sub(1 + footer_rows),
    );
    if body.height == 0 {
        return hits;
    }
    hits.body = body;
    let all = rows(
        machines,
        &stage_running,
        &reachable,
        panel,
        mode,
        current,
        body.height,
    );
    let multi = spans_machines(machines);
    let label = |studio: super::data::StudioRef<'_>| {
        studio_label(&studio.studio.id, studio.machine, local, multi)
    };
    // `all[0]` is the fixed Title — the scrolled list is the body under
    // it, so `panel.scroll` and the metrics index body rows only.
    let heights: Vec<u16> = all.iter().skip(1).map(|r| r.height()).collect();
    let metrics = list_scroll_metrics(&heights, &[], body.height, panel.scroll);
    let start = metrics.max_offset_from_bottom - metrics.offset_from_bottom;
    hits.metrics = Some(metrics);
    hits.viewport_rows = metrics.viewport_rows;
    let show_bar = metrics.max_offset_from_bottom > 0;
    let content_width = body.width - u16::from(show_bar && body.width > 1);

    let dim = Style::default().fg(palette.overlay0);
    let selected = panel.selected.as_ref();
    let studio_refs = super::data::studios(machines);
    let selected_studio = super::rows_fold::selected_studio(&studio_refs, panel, current);
    let frame_of = |studio: &StudioKey| {
        frame_style(
            lot_focused && selected_studio.as_ref() == Some(studio),
            palette,
        )
    };
    let mut y = body.y;
    for row in all.iter().skip(1 + start) {
        let height = row.height();
        if y + height > body.y + body.height {
            break; // clip: partial rows never draw
        }
        let rect = Rect::new(body.x, y, content_width, height);
        let target = row.target();
        // The selection paints the row it names — a block's `Studio`
        // target shows on its top border only — with `selection_bg`,
        // a bold `text` foreground and the accent `▌` at the row's
        // left edge (inside the block border on a person's row). An
        // unfocused panel (or one dimmed behind an active modal)
        // keeps the remembered row but draws none of it: no
        // highlight, no `▌`.
        let paints = lot_focused
            && selected == Some(&target)
            && matches!(
                row,
                Row::BlockTop { .. }
                    | Row::Person { .. }
                    | Row::Task { .. }
                    | Row::OutwardCard { .. }
                    | Row::DesignHeader { .. }
                    | Row::DesignPreview { .. }
                    | Row::DesignActions { .. }
                    | Row::AcceptedFold { .. }
                    | Row::ClosedFold { .. }
                    | Row::Stacked(_)
                    | Row::Strip { .. }
                    | Row::Talk { .. }
                    | Row::Input { .. }
                    | Row::ReopenProducer { .. }
            );
        if paints && !matches!(row, Row::Input { .. }) {
            // A block-interior row's `│` borders keep the border
            // colour on the panel background: the highlight lives
            // inside them.
            let inner = if matches!(
                row,
                Row::Person { .. }
                    | Row::Task { .. }
                    | Row::OutwardCard { .. }
                    | Row::DesignHeader { .. }
                    | Row::DesignPreview { .. }
                    | Row::DesignActions { .. }
                    | Row::AcceptedFold { .. }
                    | Row::ClosedFold { .. }
                    | Row::Talk { .. }
                    | Row::ReopenProducer { .. }
            ) {
                Rect {
                    x: rect.x + 1,
                    width: rect.width.saturating_sub(2),
                    ..rect
                }
            } else {
                rect
            };
            buf.set_style(inner, glyphs::selection(palette));
        }
        match row {
            Row::Title => {}
            Row::StageDown { machine } => {
                // `◌ host-b · stage stopped · s start` once several
                // machines answered; bare on the only machine.
                let text = match if multi { machine.or(local) } else { None } {
                    Some(name) => format!("◌ {name} · stage stopped · s start"),
                    None => "◌ stage stopped · s start".to_string(),
                };
                put_text(
                    buf,
                    rect.x,
                    rect.y,
                    rect.width,
                    &truncate_end(&text, rect.width as usize),
                    Style::default().fg(palette.peach),
                );
            }
            Row::ServerStopped { cmd } => {
                put_text(
                    buf,
                    rect.x,
                    rect.y,
                    rect.width,
                    &truncate_end(&format!("◌ {cmd}"), rect.width as usize),
                    Style::default().fg(palette.peach),
                );
            }
            Row::Empty => {
                put_text(buf, rect.x, rect.y, rect.width, "no studios", dim);
            }
            Row::StoppedLedger { count, .. } => {
                // W1b: the tree lists studios the lot could not see —
                // say how many, and that `s` starts the stage they wait on.
                let full = format!("{count} studios on stopped stages — s to start");
                let mid = format!("{count} studios on stopped stages · s start");
                let text = if rect.width >= display_width(&full) as u16 {
                    full
                } else if rect.width >= display_width(&mid) as u16 {
                    mid
                } else {
                    format!("{count} on stopped stages · s start")
                };
                put_text(
                    buf,
                    rect.x,
                    rect.y,
                    rect.width,
                    &truncate_end(&text, rect.width as usize),
                    Style::default().fg(palette.peach),
                );
            }
            Row::Offline { machine } => {
                put_text(
                    buf,
                    rect.x,
                    rect.y,
                    rect.width,
                    &truncate_end(&format!("× {machine} unreachable"), rect.width as usize),
                    Style::default().fg(palette.red),
                );
            }
            Row::BlockTop { studio, health } => {
                let is_selected = lot_focused && selected_studio.as_ref() == Some(&studio.key());
                let frame = frame_of(&studio.key());
                draw_top(
                    buf,
                    rect,
                    *studio,
                    &label(*studio),
                    *health,
                    is_selected,
                    mark == Some(&studio.key()),
                    frame,
                    palette,
                );
            }
            Row::Owed { studio, kind, text } => {
                draw_owed(buf, rect, *kind, text, frame_of(&studio.key()), palette);
            }
            Row::Usage { studio } => {
                draw_usage(buf, rect, *studio, frame_of(&studio.key()), palette);
            }
            Row::BlockBottom(studio) => {
                let edge = frame_of(&studio.key());
                put_text(buf, rect.x, rect.y, 1, "╰", edge);
                put_text(
                    buf,
                    rect.x + 1,
                    rect.y,
                    rect.width.saturating_sub(2),
                    &"─".repeat(rect.width.saturating_sub(2) as usize),
                    edge,
                );
                put_text(buf, rect.x + rect.width - 1, rect.y, 1, "╯", edge);
            }
            Row::Person { studio, role } => {
                hits.working_drawn |= matches!(Person::of(role), Person::Working);
                draw_person(
                    buf,
                    rect,
                    role,
                    frame_of(&studio.key()),
                    panel.spin,
                    now_ms,
                    palette,
                );
            }
            Row::CalmSummary {
                studio,
                idle_count,
                standby_badges,
            } => {
                draw_calm_summary(
                    buf,
                    rect,
                    *idle_count,
                    standby_badges,
                    frame_of(&studio.key()),
                    palette,
                );
            }
            Row::Divider { studio } => {
                let edge = frame_of(&studio.key());
                if rect.width >= 2 {
                    put_text(buf, rect.x, rect.y, 1, "├", edge);
                    put_text(
                        buf,
                        rect.x + 1,
                        rect.y,
                        rect.width.saturating_sub(2),
                        &"─".repeat(rect.width.saturating_sub(2) as usize),
                        edge,
                    );
                    put_text(buf, rect.x + rect.width - 1, rect.y, 1, "┤", edge);
                }
            }
            Row::CompactStudio { studio, kind } => {
                draw_compact_studio(buf, rect, *studio, *kind, &label(*studio), now_ms, palette);
            }
            Row::Task { studio, task } => {
                draw_task(buf, rect, *studio, task, frame_of(&studio.key()), palette);
            }
            Row::OutwardCard { studio, card } => {
                draw_outward_card(buf, rect, card, frame_of(&studio.key()), palette);
            }
            Row::DesignHeader { studio, design } => {
                super::draw_design::draw_header(
                    buf,
                    rect,
                    design,
                    paints,
                    frame_of(&studio.key()),
                    palette,
                );
            }
            Row::DesignPreview { studio, text } => {
                super::draw_design::draw_preview(
                    buf,
                    rect,
                    text,
                    paints,
                    frame_of(&studio.key()),
                    palette,
                );
            }
            Row::DesignActions {
                studio,
                suggest_skip,
            } => {
                super::draw_design::draw_actions(
                    buf,
                    rect,
                    *suggest_skip,
                    paints,
                    frame_of(&studio.key()),
                    palette,
                );
            }
            Row::AcceptedFold { studio, open } => {
                draw_fold(buf, rect, *studio, *open, frame_of(&studio.key()), palette);
            }
            Row::ClosedFold { studio, open } => {
                draw_closed_fold(buf, rect, *studio, *open, frame_of(&studio.key()), palette);
            }
            Row::Drawer { studio, text, .. } => {
                draw_drawer(buf, rect, text, frame_of(&studio.key()), palette);
            }
            Row::Talk { studio, line } => {
                draw_talk(buf, rect, line, paints, frame_of(&studio.key()), palette);
            }
            Row::InputDivider { studio } => {
                let edge = frame_of(&studio.key());
                let rule = Style::default().fg(elev.border_muted).bg(elev.surface);
                put_text(buf, rect.x, rect.y, 1, "├", edge);
                if rect.width > 2 {
                    put_text(
                        buf,
                        rect.x + 1,
                        rect.y,
                        rect.width.saturating_sub(2),
                        &"─".repeat(rect.width.saturating_sub(2) as usize),
                        rule,
                    );
                }
                if rect.width > 1 {
                    put_text(buf, rect.x + rect.width - 1, rect.y, 1, "┤", edge);
                }
            }
            Row::Input { studio } => {
                let producer_mark = crate::action::mark::role(&studio.key(), "producer");
                let tell_in_flight = pending
                    .pending_label(&producer_mark)
                    .is_some_and(|l| l.starts_with("telling "));
                let producer_working = studio.studio.roles.iter().any(|r| {
                    r.role == "producer"
                        && r.state == "live"
                        && r.work == Some(backlot_model::summary::Work::Working)
                });
                let awaiting_reply = studio
                    .studio
                    .exchange
                    .last()
                    .is_some_and(|e| e.who == "user");
                let cue = if studio.studio.pending_tell || tell_in_flight {
                    Some(("queued", palette.peach))
                } else if awaiting_reply && producer_working {
                    Some(("thinking", palette.accent))
                } else {
                    None
                };
                hits.working_drawn |= cue.is_some();
                // This studio's own draft only — another block's
                // line never paints its text here (R3-B3). An empty
                // stand-in keeps the borrow cheap while it rests.
                let empty;
                let draft = match panel.draft(&studio.key()) {
                    Some(draft) => draft,
                    None => {
                        empty = crate::chrome::text_editor::TextEditor::new("", false);
                        &empty
                    }
                };
                let composing = paints && panel.editing;
                hits.cursor = draw_input(
                    buf,
                    rect,
                    draft,
                    paints,
                    composing,
                    cue,
                    panel.spin,
                    frame_of(&studio.key()),
                    palette,
                )
                .or(hits.cursor.take());
            }
            Row::ReopenProducer { studio } => {
                draw_reopen(buf, rect, frame_of(&studio.key()), palette);
            }
            Row::Stacked(studio) => {
                draw_stacked(
                    buf,
                    rect,
                    *studio,
                    &label(*studio),
                    selected_studio.as_ref() == Some(&studio.key()),
                    lot_focused,
                    panel.spin,
                    palette,
                    &mut hits,
                );
            }
            Row::Strip { studio, health } => {
                draw_strip(
                    buf,
                    rect,
                    *studio,
                    *health,
                    selected_studio.as_ref() == Some(&studio.key()),
                    lot_focused,
                    mark == Some(&studio.key()),
                    multi,
                    local,
                    palette,
                );
            }
            Row::StripRole { role, .. } => {
                draw_strip_role(buf, rect, role, palette);
            }
            Row::StripSummary {
                studio,
                stage,
                reachable,
            } => {
                draw_strip_summary(buf, rect, *studio, *stage, *reachable, palette);
            }
            Row::SetupHint => {
                put_text(
                    buf,
                    rect.x,
                    rect.y,
                    rect.width,
                    &truncate_end("⏎ hollywood setup", rect.width as usize),
                    dim,
                );
            }
            Row::Update { text } => {
                put_text(
                    buf,
                    rect.x,
                    rect.y,
                    rect.width,
                    &truncate_end(text, rect.width as usize),
                    dim,
                );
            }
        }
        if paints && !matches!(row, Row::Input { .. }) {
            // `▌` over the row's left edge — inside the block border
            // on a person's row — after the content, so nothing
            // redraws over it.
            let bar_x = if matches!(
                row,
                Row::Person { .. }
                    | Row::Task { .. }
                    | Row::OutwardCard { .. }
                    | Row::DesignHeader { .. }
                    | Row::DesignPreview { .. }
                    | Row::DesignActions { .. }
                    | Row::AcceptedFold { .. }
                    | Row::ClosedFold { .. }
                    | Row::Talk { .. }
                    | Row::ReopenProducer { .. }
            ) {
                rect.x + 1
            } else {
                rect.x
            };
            put_text(
                buf,
                bar_x,
                rect.y,
                1,
                glyphs::SELECTION_BAR,
                Style::default().bg(palette.selection_bg).fg(palette.accent),
            );
        }
        // S3's pending mark — `⠋ <label>…` while the row's action
        // runs (§3.1).
        if super::pending::mark(buf, rect, row, &target, pending, panel.spin, dim) {
            hits.working_drawn = true;
        }
        hits.rows.push((rect, target));
        y += height;
    }

    if show_bar {
        hits.scrollbar = Rect::new(body.x + body.width - 1, body.y, 1, body.height);
        render_list_scrollbar(buf, hits.scrollbar, metrics, palette);
    }
    hits
}

// The frame's colour and the `│` edges live in `draw_blocks` — the
// error/next/divider/bottom rows share them with the painters there.
