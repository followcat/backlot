//! The Call Sheet's renderer (S1): the film-strip band dropped from
//! the top over the whole width — sprocket rows, the title row, one
//! frame per studio (or the chip strip), the headline, the waiting
//! items with their action chips and detail lines, the `tell
//! hollywood` line and the hint row. It never reserves a row or a
//! column: the pane layout is never recomputed for it.
//!
//! The frames live in `frames.rs`, the chip strip in `chips.rs`,
//! the waiting rows in `band.rs`, the height ladder in `plan.rs`;
//! this file owns the band, the title row, the scroll window and
//! the hit map.

use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::{Modifier, Style};

use super::plan;
use super::rows::{self, Row, Target};
use super::sheet::{Status, Wait};
use super::view;
use super::{Hollywood, LayerMode, LayerOpts, band, chips, frames, hints, items, title};
use crate::chrome::palette::Palette;
use crate::chrome::render::put_text;
use crate::chrome::scroll::{ScrollMetrics, list_scroll_metrics};
use crate::chrome::text::display_width;
use crate::panel::data::{MachineSummary, spans_machines};

/// The title row's mark.
pub(crate) const MARK: &str = "✦  H O L L Y W O O D";
/// The title row's left margin.
const MARGIN: u16 = 2;

/// What the mouse needs: the bar's whole rect, the title row, the
/// frames and band rows with their targets, the input line, the
/// waiting list's scroll metrics and the open view's map.
#[derive(Default)]
pub(crate) struct HollywoodRender {
    /// The band's rect — the click boundary.
    pub area: Rect,
    /// The title row (expanded).
    pub title: Rect,
    /// The frames and band rows (expanded).
    pub body: Rect,
    /// Hit rows with their targets — the title, the frames/chips
    /// and the band rows alike (S1's mouse selects through them).
    pub rows: Vec<(Rect, Target)>,
    /// A waiting row's action chips (S5 §2): the chip's rect, its
    /// row's target and its key — a chip click selects its row and
    /// runs the key's action.
    pub chips: Vec<(Rect, Target, char)>,
    /// Full horizontal + vertical chip area per `Wait` row so clicks on
    /// the 1-cell gap between chips select the row without triggering
    /// row activation.
    pub chips_zones: Vec<(Rect, Target)>,
    /// The input line (expanded, an agent only).
    pub input: Rect,
    /// The waiting list's scroll metrics — the wheel and PageUp/Down.
    pub metrics: Option<ScrollMetrics>,
    /// The body's scrollbar track. The film-strip design draws none,
    /// so this stays empty; the wheel and PageUp/Down still read it.
    pub scrollbar: Rect,
    /// Waiting rows visible at once.
    pub viewport_rows: usize,
    /// Frames sit before the drawn window.
    pub more_left: bool,
    /// Frames sit after it.
    pub more_right: bool,
    /// The open detail view's hit map.
    pub view: Option<view::ViewRender>,
    /// The input editor's cursor while the line is editing.
    pub cursor: Option<crate::chrome::help::CursorState>,
    /// Whether an animated spinner mark was drawn this frame.
    pub working_drawn: bool,
    /// Whether the 1-row external bottom drop shadow rendered below `area`.
    pub shadow_drawn: bool,
}

impl HollywoodRender {
    /// The point sits anywhere on the band.
    pub fn contains(&self, point: (u16, u16)) -> bool {
        self.area.contains(ratatui::layout::Position {
            x: point.0,
            y: point.1,
        })
    }
}

/// Render the bar into the frame. `machines` is every machine's last
/// good `backlot` document — the frames and the waiting items span
/// them all while the headline, the cards and the input line stay
/// this core's. `status` answers each machine's connectivity and
/// stage liveness; `local` is this core's display name.
#[allow(clippy::too_many_arguments)]
pub(crate) fn render(
    buf: &mut Buffer,
    frame: Rect,
    machines: &[MachineSummary],
    local: Option<&str>,
    status: &Status<'_>,
    hollywood: &Hollywood,
    opts: LayerOpts,
    palette: &Palette,
) -> HollywoodRender {
    match hollywood.mode {
        // The collapsed bar is the lot pill in the chrome's own row —
        // the floating strip is gone; Reader (`CardView`) renders as
        // a top-level dimmed modal in `draw_overlays` (`R3-M2`).
        LayerMode::Hidden | LayerMode::Collapsed => HollywoodRender::default(),
        LayerMode::Expanded => render_band(
            buf, frame, machines, local, status, hollywood, opts, palette,
        ),
    }
}

/// The expanded band: the plan, the frames or chips, the headline,
/// the waiting rows' window, the tell line, the hints and the two
/// sprocket rows, all over `surface_dim`.
#[allow(clippy::too_many_arguments)]
fn render_band(
    buf: &mut Buffer,
    frame: Rect,
    machines: &[MachineSummary],
    local: Option<&str>,
    status: &Status<'_>,
    hollywood: &Hollywood,
    opts: LayerOpts,
    palette: &Palette,
) -> HollywoodRender {
    if frame.width == 0 || frame.height == 0 {
        return HollywoodRender::default();
    }
    let multi = spans_machines(machines);
    let local_label = local.unwrap_or("Local");
    let live_prompt = rows::hollywood_asking(machines);
    let all_frames = rows::frames(machines);
    let (waits, band_rows) = rows::band_rows(machines, status, opts.agent);
    let plan = plan::plan(frame, &waits, &band_rows, hollywood, opts);
    let selected_wait = hollywood
        .selected
        .as_ref()
        .and_then(|t| waits.iter().position(|w| &w.target() == t));

    // The scroll window: `scroll` picks the first waiting row drawn;
    // `shown` how many. Keep the selected row inside the window.
    let scroll = hollywood
        .scroll
        .min(waits.len().saturating_sub(plan.shown.max(1)));
    let scroll = selected_wait.map_or(scroll, |sel| {
        if sel < scroll {
            sel
        } else if sel >= scroll + plan.shown {
            (sel + 1).saturating_sub(plan.shown)
        } else {
            scroll
        }
    });

    let area = Rect::new(
        frame.x,
        frame.y,
        frame.width,
        plan.height().min(frame.height),
    );
    let activity = if opts.agent {
        rows::hollywood_activity(machines, hollywood.pending.as_deref())
    } else {
        rows::HollywoodActivity::Idle
    };
    let mut hits = HollywoodRender {
        area,
        viewport_rows: plan.shown,
        working_drawn: activity.is_active(),
        ..HollywoodRender::default()
    };
    if area.is_empty() {
        return hits;
    }
    // Reset every cell the band covers — pane borders and titles must
    // not show through its blanks.
    for y in area.y..area.y + area.height {
        for x in area.x..area.x + area.width {
            buf[(x, y)].reset();
        }
    }
    let elev = palette.elevation();
    buf.set_style(area, Style::default().bg(elev.surface));

    let studios_n = all_frames
        .iter()
        .filter(|f| matches!(f, rows::Frame::Studio(_)))
        .count();
    let sel_ref = (!opts.suppress_highlights)
        .then_some(hollywood.selected.as_ref())
        .flatten();
    let studios_focused =
        sel_ref.is_some_and(|t| matches!(t, Target::Studio(_) | Target::NewStudio | Target::Title));
    let wait_focused = sel_ref.is_some_and(|t| {
        matches!(
            t,
            Target::Prompt(..)
                | Target::Role(..)
                | Target::Tell(..)
                | Target::Upgrade(..)
                | Target::Stage(..)
                | Target::Card(..)
                | Target::OutwardCard(..)
                | Target::Ready(..)
                | Target::Hollywood
        )
    });
    let hollywood_focused = sel_ref.is_some_and(|t| {
        matches!(
            t,
            Target::Headline
                | Target::Talk(_)
                | Target::HollywoodLive
                | Target::Input
                | Target::Setup
        )
    });

    let band_wait_total = band_rows
        .iter()
        .filter(|r| matches!(r, Row::Wait(_)))
        .count();
    let scroll_range = (band_wait_total > plan.shown && plan.shown > 0)
        .then(|| (scroll + 1, (scroll + plan.shown).min(band_wait_total)));
    let title_scroll = (!plan.waiting_rule && !plan.hints)
        .then_some(scroll_range)
        .flatten();

    let mut y = area.y;
    if plan.top_sprocket {
        title::draw_sprocket(buf, area, y, '▄', palette);
        y += 1;
    }
    if plan.title {
        title::draw_title(
            buf,
            area,
            y,
            machines,
            &waits,
            opts.agent,
            activity,
            hollywood.spin,
            title_scroll,
            palette,
            &mut hits,
        );
        y += 1;
    }
    if plan.blank_1 {
        y += 1;
    }
    if plan.studios_rule {
        super::zone::draw_studios_rule(buf, area, y, studios_n, studios_focused, palette);
        y += 1;
    }
    if plan.frames_h == frames::FRAME_H {
        let rect = Rect::new(area.x, y, area.width, plan.frames_h);
        let drawn = frames::render(
            buf,
            rect,
            &all_frames,
            frames::FramesView {
                selected: hollywood.selected.as_ref(),
                active_studio: hollywood.active_studio.as_ref(),
                scroll: hollywood.frame_scroll,
                mobile: opts.mobile,
                local,
                multi,
                status,
                suppress_highlights: opts.suppress_highlights,
            },
            palette,
        );
        hits.rows.extend(drawn.hits);
        hits.more_left = drawn.more_left;
        hits.more_right = drawn.more_right;
        hits.body = rect;
        y += drawn.height;
    } else if plan.frames_h == 1 {
        let rect = Rect::new(area.x, y, area.width, 1);
        let drawn = chips::render(
            buf,
            rect,
            &all_frames,
            status,
            hollywood.selected.as_ref(),
            hollywood.frame_scroll,
            local,
            multi,
            opts.suppress_highlights,
            palette,
        );
        hits.rows.extend(drawn.hits);
        hits.more_left = drawn.more_left;
        hits.more_right = drawn.more_right;
        hits.body = rect;
        y += 1;
    }
    if plan.blank_2 {
        y += 1;
    }

    let now_ms = hollywood.now_ms.unwrap_or_else(crate::update_state::now_ms);
    let user_urgent = items::any_user_urgent(&waits);
    if plan.waiting_rule && !waits.is_empty() {
        super::zone::draw_waiting_rule(
            buf,
            area,
            y,
            waits.len(),
            user_urgent,
            wait_focused,
            scroll_range.map(|(top, bot)| (top, bot.min(waits.len()))),
            palette,
        );
        y += 1;
    }

    // Upper-half lower zone: visible waiting items or calm status line.
    let mut wait_seen = 0usize;
    for row in &band_rows {
        match row {
            Row::Wait(i) => {
                let index = *i;
                let in_window = wait_seen >= scroll && wait_seen < scroll + plan.shown;
                wait_seen += 1;
                if !in_window {
                    continue;
                }
                let detail = plan.details.get(index).copied().unwrap_or(false);
                let h = 1 + u16::from(detail);
                let rect = Rect::new(area.x, y, area.width, h);
                band::draw_row(
                    buf,
                    rect,
                    row,
                    &waits,
                    hollywood,
                    local_label,
                    multi,
                    opts.mobile,
                    opts.suppress_highlights,
                    activity,
                    now_ms,
                    palette,
                    &mut hits,
                );
                y += h;
            }
            Row::Calm { .. } => {
                let rect = Rect::new(area.x, y, area.width, 1);
                band::draw_row(
                    buf,
                    rect,
                    row,
                    &waits,
                    hollywood,
                    local_label,
                    multi,
                    opts.mobile,
                    opts.suppress_highlights,
                    activity,
                    now_ms,
                    palette,
                    &mut hits,
                );
                y += 1;
            }
            _ => {}
        }
    }

    // Lower zone: `HOLLYWOOD` rule (with embedded fold target) or setup
    // hairline, then headline/talk/live/compose/setup.
    let talk_entries: Vec<&backlot_model::summary::Exchange> = band_rows
        .iter()
        .filter_map(|r| match r {
            Row::Talk(e) => Some(*e),
            _ => None,
        })
        .collect();
    let talk_total = talk_entries.len();
    let shown_talks = (plan.talk as usize).min(talk_total);
    let skipped_talks = talk_total.saturating_sub(shown_talks);
    let model_fold = band_rows.iter().find_map(|r| match r {
        Row::Fold { count, newest } => Some((*count, *newest)),
        _ => None,
    });
    let fold_badge = if skipped_talks > 0 {
        let newest = talk_entries[skipped_talks - 1].seq;
        let count = model_fold.map_or(0, |(c, _)| c) + skipped_talks;
        Some(super::zone::FoldBadge {
            count,
            newest,
            selected: sel_ref == Some(&Target::Talk(newest)),
        })
    } else {
        model_fold.map(|(count, newest)| super::zone::FoldBadge {
            count,
            newest,
            selected: sel_ref == Some(&Target::Talk(newest)),
        })
    };

    if opts.agent && plan.hollywood_rule {
        super::zone::draw_hollywood_rule(
            buf,
            area,
            y,
            hollywood_focused,
            fold_badge,
            palette,
            &mut hits,
        );
        y += 1;
    } else if !opts.agent && plan.setup_hairline {
        super::zone::draw_setup_hairline(buf, area, y, palette);
        y += 1;
    }

    let mut talk_seen = 0usize;
    for row in &band_rows {
        match row {
            Row::Headline(_) if plan.headline => {
                let rect = Rect::new(area.x, y, area.width, 1);
                band::draw_row(
                    buf,
                    rect,
                    row,
                    &waits,
                    hollywood,
                    local_label,
                    multi,
                    opts.mobile,
                    opts.suppress_highlights,
                    activity,
                    now_ms,
                    palette,
                    &mut hits,
                );
                y += 1;
            }
            Row::Talk(_) => {
                let idx = talk_seen;
                talk_seen += 1;
                if idx < skipped_talks {
                    continue;
                }
                let h = 1 + plan.talk_wrap_at(idx);
                let rect = Rect::new(area.x, y, area.width, h);
                band::draw_row(
                    buf,
                    rect,
                    row,
                    &waits,
                    hollywood,
                    local_label,
                    multi,
                    opts.mobile,
                    opts.suppress_highlights,
                    activity,
                    now_ms,
                    palette,
                    &mut hits,
                );
                y += h;
            }
            Row::Live(_) if plan.live => {
                let rect = Rect::new(area.x, y, area.width, 1);
                band::draw_row(
                    buf,
                    rect,
                    row,
                    &waits,
                    hollywood,
                    local_label,
                    multi,
                    opts.mobile,
                    opts.suppress_highlights,
                    activity,
                    now_ms,
                    palette,
                    &mut hits,
                );
                y += 1;
            }
            Row::Input | Row::Setup => {
                let rect = Rect::new(area.x, y, area.width, 1);
                band::draw_row(
                    buf,
                    rect,
                    row,
                    &waits,
                    hollywood,
                    local_label,
                    multi,
                    opts.mobile,
                    opts.suppress_highlights,
                    activity,
                    now_ms,
                    palette,
                    &mut hits,
                );
                y += 1;
            }
            _ => {}
        }
    }
    hits.metrics = Some(list_scroll_metrics(
        &vec![1; waits.len()],
        &[],
        plan.shown.max(1) as u16,
        scroll,
    ));
    if plan.hints {
        let usable = (area.width as usize).saturating_sub((MARGIN * 2) as usize);
        let tier = if hollywood.confirm.is_some() {
            hints::pairs_for_confirm(usable)
        } else {
            hints::pairs_for_row_with_handoff(
                hollywood.selected.as_ref(),
                hollywood.editing,
                &waits,
                opts.agent,
                live_prompt,
                usable,
            )
        };
        crate::chrome::render::put_hint_bar(buf, area.x + MARGIN, y, usable as u16, &tier, palette);
        if !plan.waiting_rule
            && let Some((top, bot)) = scroll_range
        {
            let mark = format!("{top}–{bot}/{}", waits.len());
            let mw = display_width(&mark) as u16;
            let mx = (area.x + area.width).saturating_sub(MARGIN + mw);
            let hint_w = crate::chrome::render::hint_bar_width(&tier);
            if mx > area.x + MARGIN + hint_w + 1 {
                put_text(
                    buf,
                    mx,
                    y,
                    mw,
                    &mark,
                    Style::default().fg(elev.muted_surface).bg(elev.surface),
                );
            }
        }
        y += 1;
    }
    if plan.bottom_sprocket {
        title::draw_sprocket(buf, area, y, '▀', palette);
    }
    if plan.shadow {
        let shadow_row = Rect::new(area.x, area.bottom(), area.width, 1);
        crate::chrome::popup_frame::draw_bottom_shadow(buf, shadow_row, frame, palette, true);
        hits.shadow_drawn = !shadow_row
            .intersection(frame)
            .intersection(buf.area)
            .is_empty();
    }
    hits
}

/// Pixel/cell gap between adjacent action chips on a waiting row.
pub(crate) const CHIP_GAP: u16 = 1;

/// A waiting row's action chips (S5 §2), painted right-aligned from
/// `x` — the focused chip lit `accent` on the selected row — and
/// recorded as full-row-height click targets: `↵` maps to the row's
/// Enter, a letter to the row's own key.
#[allow(clippy::too_many_arguments)]
pub(crate) fn paint_wait_chips(
    buf: &mut Buffer,
    y: u16,
    mut x: u16,
    row_height: u16,
    wait: &Wait<'_>,
    chips: &[items::Chip],
    compact: bool,
    selected: bool,
    focused_chip: Option<usize>,
    palette: &Palette,
    hits: &mut HollywoodRender,
) {
    let start_x = x;
    let target = wait.target();
    let hit_h = row_height.max(1);
    let elev = palette.elevation();
    for (idx, chip) in chips.iter().enumerate() {
        let text = items::chip_text(chip, compact);
        let w = display_width(&text) as u16;
        let is_focused = selected && focused_chip == Some(idx);
        let style = if is_focused {
            if elev.is_terminal_16 {
                Style::default()
                    .fg(palette.text)
                    .add_modifier(Modifier::REVERSED | Modifier::BOLD)
            } else {
                Style::default()
                    .fg(crate::studio::glyphs::contrast_fg(palette))
                    .bg(palette.accent)
                    .add_modifier(Modifier::BOLD)
            }
        } else {
            let chip_bg = if elev.is_dark {
                palette.surface0
            } else {
                elev.surface
            };
            Style::default().fg(palette.subtext0).bg(chip_bg)
        };
        put_text(buf, x, y, w, &text, style);
        let key = if chip.key == "↵" {
            '\r'
        } else {
            chip.key.chars().next().unwrap_or('\r')
        };
        hits.chips
            .push((Rect::new(x, y, w, hit_h), target.clone(), key));
        x += w;
        if idx + 1 < chips.len() {
            x += CHIP_GAP;
        }
    }
    let total_w = x.saturating_sub(start_x);
    if total_w > 0 {
        hits.chips_zones
            .push((Rect::new(start_x, y, total_w, hit_h), target));
    }
}
