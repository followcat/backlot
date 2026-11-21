//! The in-TUI Hollywood setup dialog: a small modal the no-agent
//! band's setup row, the panel's `hollywood setup` hint, `c` on a
//! configured bar and the settings overseer row all open. It probes
//! `backlot runners --json` on this machine — only `found` runners
//! are pickable — offers the runner, that runner's model catalog
//! with `auto` first (free text kept, the step-3 cell's own rules)
//! and its effort list when it has one, and applies through
//! `backlot hollywood setup` — the same write the CLI does, never a
//! second TOML path. A running stage picks the table up on its next
//! tick and starts `bl-hollywood` as before; a runner change starts
//! a new conversation and `memory.md` stays.

use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::{Modifier, Style};
use ratatui::widgets::{Clear, Widget};

use backlot_stage::config::HollywoodCfg;

use crate::chrome::help::CursorState;
use crate::chrome::palette::Palette;
use crate::chrome::render::{put_right_text, put_segment, put_text};
use crate::chrome::text::{display_width, truncate_end};
use crate::chrome::text_editor::TextEditor;
use crate::new_studio::{ModelPick, Probe, RunnerRow};

#[cfg(test)]
#[path = "tests.rs"]
mod tests;

/// The dialog's rows — `Effort` only exists while the selected
/// runner declares an effort list.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Field {
    Runner,
    Model,
    Effort,
    Apply,
}

/// The whole dialog state. The probe is generation-tagged like the
/// wizard's — a stale answer never lands.
pub(crate) struct HwSetup {
    /// `runners --json` — `found` rows are the pickable runners.
    pub runners: Probe<Vec<RunnerRow>>,
    pub runners_gen: u64,
    /// Index into the found list.
    pub runner: usize,
    /// The model value — `auto`, a catalog id or free text.
    pub model: String,
    /// `e`'s free-text field on the model row.
    pub model_edit: Option<TextEditor>,
    /// `⏎`'s catalog list on the model row — `auto` first.
    pub model_pick: Option<ModelPick>,
    /// Index into the selected runner's `efforts`.
    pub effort: usize,
    /// The focused row.
    pub field: Field,
    /// The existing table's values — reconfigure's prefill and the
    /// runner-change warning's base; `key`/`default_cast` carry over
    /// verbatim on the write. `None` on a first setup.
    pub existing: Option<HollywoodCfg>,
    /// The first probe answer seats `existing`'s values once — a
    /// `^R` refresh reseats nothing the user arranged.
    pub(crate) prefilled: bool,
}

/// One clickable cell in the last render.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum HwHit {
    Field(Field),
    /// A row in the open model list — the filtered index.
    ModelPick(usize),
}

/// The hit map the last render left: the frame (clicks outside
/// close), every registered cell and an editor's cursor.
#[derive(Default)]
pub(crate) struct HwHits {
    pub area: Rect,
    pub cells: Vec<(Rect, HwHit)>,
    pub cursor: Option<CursorState>,
}

impl HwHits {
    fn push(&mut self, rect: Rect, hit: HwHit) {
        if rect.width > 0 && rect.height > 0 {
            self.cells.push((rect, hit));
        }
    }
}

impl HwSetup {
    pub(crate) fn new(existing: Option<HollywoodCfg>) -> Self {
        HwSetup {
            runners: Probe::Pending,
            runners_gen: 0,
            runner: 0,
            model: "auto".to_string(),
            model_edit: None,
            model_pick: None,
            effort: 0,
            field: Field::Runner,
            existing,
            prefilled: false,
        }
    }

    /// The first probe answer's prefill: an existing table's runner
    /// selects itself in the found list and its model/effort seat
    /// verbatim — the cfg's own values, not the row's defaults. A
    /// missing runner keeps the first found row's defaults.
    pub(crate) fn prefill(&mut self) {
        if self.prefilled || !matches!(self.runners, Probe::Done(_)) {
            return;
        }
        self.prefilled = true;
        let Some(old) = &self.existing else {
            return;
        };
        // Compute the seats under `found`'s borrow, then write them.
        let pick = {
            let found = self.found();
            found.iter().position(|r| r.runner == old.runner).map(|i| {
                (
                    i,
                    old.effort
                        .as_ref()
                        .and_then(|e| found[i].efforts.iter().position(|v| v == e)),
                )
            })
        };
        if let Some((i, effort)) = pick {
            self.runner = i;
            self.model = old.model.clone();
            if let Some(j) = effort {
                self.effort = j;
            }
        }
    }

    /// The cfg the dialog's apply writes — the picked values plus
    /// `existing`'s untouched `key`/`default_cast`. `None` while no
    /// found runner is picked or the model is empty.
    pub(crate) fn cfg(&self) -> Option<HollywoodCfg> {
        if self.model.trim().is_empty() {
            return None;
        }
        Some(HollywoodCfg {
            runner: self.runner_row()?.runner.clone(),
            model: self.model.clone(),
            effort: self.effort_value().map(str::to_string),
            key: self.existing.as_ref().and_then(|e| e.key.clone()),
            default_cast: self.existing.as_ref().and_then(|e| e.default_cast.clone()),
        })
    }

    /// The pickable runners — detection's `found` rows only.
    pub(crate) fn found(&self) -> Vec<&RunnerRow> {
        match &self.runners {
            Probe::Done(rows) => rows.iter().filter(|r| r.found()).collect(),
            _ => Vec::new(),
        }
    }

    /// The selected runner row.
    pub(crate) fn runner_row(&self) -> Option<&RunnerRow> {
        self.found().get(self.runner).copied()
    }

    /// The selected runner's effort list.
    fn efforts(&self) -> &[String] {
        self.runner_row()
            .map(|r| r.efforts.as_slice())
            .unwrap_or(&[])
    }

    /// Whether the effort row exists for the selected runner.
    fn has_effort(&self) -> bool {
        !self.efforts().is_empty()
    }

    /// The rows in order — Effort only on a runner with a list.
    pub(crate) fn fields(&self) -> Vec<Field> {
        let mut out = vec![Field::Runner, Field::Model];
        if self.has_effort() {
            out.push(Field::Effort);
        }
        out.push(Field::Apply);
        out
    }

    /// `↑↓`/`tab`: the next row in the fields list, wrapped.
    pub(crate) fn move_field(&mut self, delta: isize) {
        let fields = self.fields();
        let index = fields.iter().position(|f| *f == self.field).unwrap_or(0) as isize;
        let next = (index + delta).rem_euclid(fields.len() as isize) as usize;
        self.field = fields[next];
    }

    /// `←→` cycles the focused row's value: a runner move reseats
    /// model to `auto` and effort to the list's head — and warns
    /// when the change means a new conversation (D14); model cycles
    /// `auto` + the catalog (+ a stray free value); effort its list.
    pub(crate) fn cycle(&mut self, delta: isize) {
        match self.field {
            Field::Runner => {
                let n = self.found().len();
                if n == 0 {
                    return;
                }
                self.runner = (self.runner as isize + delta).rem_euclid(n as isize) as usize;
                self.model = "auto".to_string();
                self.effort = 0;
                if self.field == Field::Effort && !self.has_effort() {
                    self.field = Field::Apply;
                }
            }
            Field::Model => {
                let mut options = vec!["auto".to_string()];
                if let Some(runner) = self.runner_row() {
                    options.extend(
                        runner
                            .models
                            .iter()
                            .filter(|m| m.as_str() != "auto")
                            .cloned(),
                    );
                }
                if !options.contains(&self.model) {
                    options.push(self.model.clone());
                }
                let index = options.iter().position(|m| *m == self.model).unwrap_or(0) as isize;
                let next = (index + delta).rem_euclid(options.len() as isize) as usize;
                self.model = options[next].clone();
            }
            Field::Effort => {
                let n = self.efforts().len();
                if n > 0 {
                    self.effort = (self.effort as isize + delta).rem_euclid(n as isize) as usize;
                }
            }
            Field::Apply => {}
        }
    }

    /// `⏎`/space on the model row — the catalog list, `auto` first,
    /// the current value selected.
    pub(crate) fn open_pick(&mut self) {
        if let Some(runner) = self.runner_row() {
            self.model_pick = Some(ModelPick::open(&runner.models, &self.model));
        }
    }

    /// The effort value the apply would send — `None` on a runner
    /// without a list, the list entry otherwise.
    pub(crate) fn effort_value(&self) -> Option<&str> {
        self.efforts().get(self.effort).map(String::as_str)
    }

    /// The runner change's warning — a reconfigure that reseats the
    /// runner starts a new conversation (D14); `memory.md` stays.
    pub(crate) fn runner_changed(&self) -> bool {
        self.existing
            .as_ref()
            .zip(self.runner_row())
            .is_some_and(|(old, now)| old.runner != now.runner)
    }
}

/// The dialog's outer rect — a centered card much smaller than the
/// wizard's frame; a pick list gives it a taller shape.
fn frame(band: Rect, pick_open: bool) -> Option<Rect> {
    let w = band.width.saturating_sub(4).min(56);
    let h = if pick_open { 16 } else { 11 }.min(band.height.saturating_sub(2));
    if w < 26 || h < 8 {
        return None;
    }
    Some(Rect::new(
        band.x + (band.width - w) / 2,
        band.y + (band.height - h) / 2,
        w,
        h,
    ))
}

/// The dialog's draw: the caller dims the backdrop; the frame is
/// the S3 modal's own inside `band` so a beside dock is never
/// crossed (A6.2).
pub(crate) fn render(b: &mut Buffer, d: &HwSetup, p: &Palette, band: Rect) -> HwHits {
    let mut hits = HwHits::default();
    let Some(outer) = frame(band, d.model_pick.is_some()) else {
        put_text(
            b,
            b.area.x,
            b.area.y,
            b.area.width,
            "hollywood setup: terminal too small",
            Style::default().fg(p.red),
        );
        return hits;
    };
    // The dimmed backdrop is the caller's; the dead panes' frozen
    // screens behind must not keep their modifiers under the panel's
    // patch fill — clear the dialog's own rect before the frame.
    Clear.render(outer, b);
    let Some(inner) = crate::chrome::dialogs::panel(b, outer, p.accent, p.panel_bg) else {
        return hits;
    };
    hits.area = outer;
    let title = if d.existing.is_some() {
        " Hollywood · reconfigure "
    } else {
        " Hollywood setup "
    };
    put_text(
        b,
        inner.x,
        inner.y,
        inner.width,
        title,
        Style::default()
            .fg(p.accent)
            .bg(p.panel_bg)
            .add_modifier(Modifier::BOLD),
    );
    put_right_text(
        b,
        inner,
        inner.y,
        "esc cancel",
        Style::default().fg(p.overlay0).bg(p.panel_bg),
    );
    let body = Rect::new(
        inner.x + 1,
        inner.y + 2,
        inner.width.saturating_sub(2),
        inner.height.saturating_sub(3),
    );
    if let Some(pick) = &d.model_pick {
        draw_pick(b, body, d, pick, p, &mut hits);
        return hits;
    }
    match &d.runners {
        Probe::Pending => {
            put_text(
                b,
                body.x,
                body.y,
                body.width,
                "detecting runners…",
                Style::default().fg(p.overlay0).bg(p.panel_bg),
            );
            return hits;
        }
        Probe::Failed(e) => {
            put_text(
                b,
                body.x,
                body.y,
                body.width,
                &truncate_end(e, body.width as usize),
                Style::default().fg(p.red).bg(p.panel_bg),
            );
            return hits;
        }
        Probe::Done(_) if d.found().is_empty() => {
            put_text(
                b,
                body.x,
                body.y,
                body.width,
                "no runners installed on this machine",
                Style::default().fg(p.peach).bg(p.panel_bg),
            );
            put_text(
                b,
                body.x,
                body.y + 1,
                body.width,
                "install one — `backlot runners` lists what is looked for",
                Style::default().fg(p.overlay0).bg(p.panel_bg),
            );
            return hits;
        }
        Probe::Done(_) => {}
    }
    let fields = d.fields();
    let mut y = body.y;
    for field in fields {
        if y >= body.bottom().saturating_sub(1) {
            break;
        }
        let line = Rect::new(body.x, y, body.width, 1);
        let selected = field == d.field;
        let bg = if selected { p.surface0 } else { p.panel_bg };
        if selected {
            b.set_style(line, Style::default().bg(p.surface0));
            put_text(b, body.x, y, 1, "▸", Style::default().fg(p.accent).bg(bg));
        }
        let (name, value) = match field {
            Field::Runner => (
                "runner",
                d.runner_row()
                    .map(|r| r.runner.clone())
                    .unwrap_or_else(|| "-".into()),
            ),
            Field::Model => ("model", d.model.clone()),
            Field::Effort => ("effort", d.effort_value().unwrap_or("-").to_string()),
            Field::Apply => ("apply", String::new()),
        };
        if field == Field::Apply {
            put_text(
                b,
                body.x + 1,
                y,
                body.width,
                "apply",
                Style::default()
                    .fg(if selected { p.accent } else { p.teal })
                    .bg(bg)
                    .add_modifier(Modifier::BOLD),
            );
            hits.push(line, HwHit::Field(field));
            y += 1;
            continue;
        }
        put_text(
            b,
            body.x + 1,
            y,
            8,
            name,
            Style::default().fg(p.overlay0).bg(bg),
        );
        let cell = Rect::new(body.x + 9, y, body.width.saturating_sub(9), 1);
        // `e`'s free-text field replaces the model value while open.
        if selected && field == Field::Model && d.model_edit.is_some() {
            b.set_style(cell, Style::default().bg(p.surface0));
            if let Some(edit) = &d.model_edit
                && let Some(cursor) = crate::chrome::text_editor::render(
                    b,
                    cell,
                    edit,
                    Style::default().fg(p.text).bg(p.surface0),
                )
            {
                hits.cursor = Some(cursor);
            }
        } else {
            let (text, style) = if selected {
                (
                    format!("‹ {value} ›"),
                    Style::default()
                        .fg(p.accent)
                        .bg(bg)
                        .add_modifier(Modifier::BOLD),
                )
            } else {
                (value, Style::default().fg(p.text).bg(bg))
            };
            put_segment(
                b,
                cell.x,
                y,
                cell.right(),
                &truncate_end(&text, cell.width as usize),
                style,
            );
        }
        hits.push(line, HwHit::Field(field));
        y += 1;
    }
    // The runner-change warning (D14): a reconfigure that reseats
    // the runner starts a new conversation — memory.md stays.
    if d.runner_changed() && y < body.bottom().saturating_sub(1) {
        put_text(
            b,
            body.x + 1,
            y,
            body.width,
            "new runner, new conversation — memory.md stays",
            Style::default().fg(p.peach).bg(p.panel_bg),
        );
    }
    let hint = " ↑↓ field  ←→ value  ⏎ list · apply  e text  esc cancel";
    put_text(
        b,
        inner.x,
        inner.bottom().saturating_sub(1),
        inner.width,
        &truncate_end(hint, inner.width as usize),
        Style::default().fg(p.overlay0).bg(p.panel_bg),
    );
    hits
}

/// The open model list — the step-3 picker's own shape: `auto`
/// first marked `· runner default`, the live filter in the head,
/// the current value wearing `‹ ›`.
fn draw_pick(
    b: &mut Buffer,
    body: Rect,
    d: &HwSetup,
    pick: &ModelPick,
    p: &Palette,
    hits: &mut HwHits,
) {
    let runner = d.runner_row().map(|r| r.runner.as_str()).unwrap_or("-");
    let head_style = Style::default()
        .fg(p.accent)
        .bg(p.panel_bg)
        .add_modifier(Modifier::BOLD);
    let head = truncate_end(
        &format!("Model for hollywood · {runner}:"),
        body.width as usize,
    );
    let head_w = display_width(&head) as u16;
    let mut y = body.y;
    put_text(b, body.x, y, body.width, &head, head_style);
    let tail_x = body.x + head_w;
    if pick.filter.as_str().is_empty() {
        let tail_w = body.right().saturating_sub(tail_x);
        put_text(
            b,
            tail_x,
            y,
            tail_w,
            &truncate_end("  (type to filter)", tail_w as usize),
            head_style,
        );
    } else {
        let x = put_segment(b, tail_x, y, body.right(), "  /", head_style);
        let w = body.right().saturating_sub(x);
        if w > 0
            && let Some(cursor) = crate::chrome::text_editor::render(
                b,
                Rect::new(x, y, w, 1),
                &pick.filter,
                Style::default().fg(p.text).bg(p.panel_bg),
            )
        {
            hits.cursor = Some(cursor);
        }
    }
    y += 1;
    let visible = pick.visible();
    if visible.is_empty() {
        put_text(
            b,
            body.x + 1,
            y,
            body.width,
            "nothing matches the filter",
            Style::default().fg(p.overlay0).bg(p.panel_bg),
        );
        return;
    }
    let window = body.bottom().saturating_sub(y) as usize;
    if window == 0 {
        return;
    }
    let scroll = pick
        .scroll
        .min(visible.len().saturating_sub(window))
        .min(pick.sel)
        .max(pick.sel.saturating_sub(window.saturating_sub(1)));
    for (vi, option) in visible.iter().skip(scroll).take(window).enumerate() {
        let i = scroll + vi;
        let line = Rect::new(body.x, y, body.width, 1);
        let selected = i == pick.sel;
        let bg = if selected { p.surface0 } else { p.panel_bg };
        if selected {
            b.set_style(line, Style::default().bg(p.surface0));
            put_text(b, body.x, y, 1, "▸", Style::default().fg(p.accent).bg(bg));
        }
        let text = if *option == d.model {
            format!("‹ {option} ›")
        } else {
            (*option).to_string()
        };
        let x = put_segment(
            b,
            body.x + 2,
            y,
            body.right(),
            &format!(
                " {}",
                truncate_end(&text, (body.width as usize).saturating_sub(3))
            ),
            Style::default().fg(p.text).bg(bg),
        );
        if *option == "auto" {
            put_segment(
                b,
                x,
                y,
                body.right(),
                " · runner default",
                Style::default().fg(p.overlay0).bg(bg),
            );
        }
        hits.push(line, HwHit::ModelPick(i));
        y += 1;
        if y >= body.bottom() {
            break;
        }
    }
}
