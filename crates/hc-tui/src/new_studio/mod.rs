//! The native New Studio dialog (S4) — a four-step overlay that
//! replaces the cooked-stdin wizard popup: machine & repository,
//! id & title, cast, and the dry-run check with start. Every
//! machine question goes through `backlot … --json` run locally or
//! over `RemoteRunner` (`terminal.run on:`), so a remote repo path
//! is never checked against the local disk (A6.3).
//!
//! `model` holds the parsed documents and the cast table; each
//! `stepN` file draws one step inside the shared frame this module
//! owns — title, step tabs, hint row and the dim backdrop are the
//! `Ctrl+B ?` overlay's.

mod model;
mod step1;
mod step2;
mod step3;
mod step4;

pub(crate) use model::{
    CastTable, MachineRow, MachineState, ModelPick, PathShape, Probe, RepoEntry, RepoKind, Repos,
    RunnerRow, Step, Step1Pane, id_error, parse_repos, parse_runners, studio_id, title_prefill,
};

use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::{Modifier, Style};

use crate::chrome::help::CursorState;
use crate::chrome::palette::Palette;
use crate::chrome::render::{put_right_text, put_text};
use crate::chrome::text::{display_width, fit_hints};
use crate::chrome::text_editor::TextEditor;

/// Step 2's focusable fields — `Mode` is a two-value toggle, not
/// a text field: `←`/`→`/space flips it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Step2Field {
    Id,
    Title,
    Mode,
}

/// The whole dialog. All probes are generation-tagged: a stale
/// answer — a machine switch or a `^R` refresh — never lands.
pub(crate) struct NewStudio {
    pub step: Step,
    /// Local plus every attached machine, with its probe.
    pub machines: Vec<MachineRow>,
    pub machine_sel: usize,
    /// A `^R` re-check in flight: rows keep their last verdict.
    pub refreshing: bool,
    /// Generation for the machine-version probes.
    pub machines_gen: u64,
    /// Step 1's active half.
    pub pane: Step1Pane,
    /// The repository browser for the selected machine.
    pub repos: Repos,
    /// Step 2's fields — the id is replace-on-type like a slug.
    pub id: TextEditor,
    pub title: TextEditor,
    pub field: Step2Field,
    /// The studio's production mode (R14) — `standard` default.
    pub mode: backlot_model::event::StudioMode,
    /// The repo path the id/title prefills came from — re-picking
    /// the same repo keeps the user's edits (Esc-back's rule).
    pub prefilled_for: Option<String>,
    /// Studio ids already on the selected machine (the dup check).
    pub studios: Vec<String>,
    /// Step 3: the detected runners and the editable cast.
    pub runners: Probe<Vec<RunnerRow>>,
    pub runners_gen: u64,
    pub cast: CastTable,
    /// `e` on a model cell turns it into a free-text field.
    pub model_edit: Option<TextEditor>,
    /// `⏎`/`space` on a model cell opens the runner's catalog list.
    pub model_pick: Option<ModelPick>,
    /// Step 4: the dry run and the create call.
    pub dry: Probe<String>,
    pub dry_gen: u64,
    /// `[x] start stage` — checked by default like the mockup.
    pub start_stage: bool,
    /// `[ ] no git` — a plain-directory pick's degraded mode (D4),
    /// only offered on step 4's launch check.
    pub no_git: bool,
    pub creating: bool,
    /// The create call's own generation — one in flight at most.
    pub create_gen: u64,
    /// The finished create's error line; success closes instead.
    pub create_error: Option<String>,
}

/// One clickable cell in the last render — the App's mouse dispatch
/// resolves the hit back to a row or cell.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum NsHit {
    Machine(usize),
    Repo(usize),
    StepTab(Step),
    IdField,
    TitleField,
    ModeField,
    CastCell {
        row: usize,
        col: usize,
    },
    /// A row in the open model list — the index into its filtered
    /// rows, like `Repo`'s.
    ModelPick(usize),
    StartToggle,
    NoGitToggle,
    Create,
}

/// The hit map the last render left: outer frame (clicks outside
/// close), plus every registered cell.
#[derive(Default)]
pub(crate) struct NsHits {
    pub area: Rect,
    pub cells: Vec<(Rect, NsHit)>,
    pub cursor: Option<CursorState>,
}

impl NsHits {
    fn push(&mut self, rect: Rect, hit: NsHit) {
        if rect.width > 0 && rect.height > 0 {
            self.cells.push((rect, hit));
        }
    }
}

impl NewStudio {
    /// A fresh dialog on step 1. `preselect` is the machine the
    /// caller chose (the sidebar's `new · <machine>`); the default
    /// is Local. The machine rows start `Pending` — the App fills
    /// them from the endpoint list, then probes each for
    /// `backlot --version`.
    pub(crate) fn new() -> Self {
        NewStudio {
            step: Step::MachineRepo,
            machines: Vec::new(),
            machine_sel: 0,
            refreshing: false,
            machines_gen: 0,
            pane: Step1Pane::Machines,
            repos: Repos::default(),
            id: TextEditor::new("", false),
            title: TextEditor::new("", false),
            field: Step2Field::Id,
            mode: backlot_model::event::StudioMode::Standard,
            prefilled_for: None,
            studios: Vec::new(),
            runners: Probe::Pending,
            runners_gen: 0,
            cast: CastTable {
                rows: Vec::new(),
                sel: 0,
                col: 0,
                custom: None,
                custom_error: None,
            },
            model_edit: None,
            model_pick: None,
            dry: Probe::Pending,
            dry_gen: 0,
            start_stage: true,
            no_git: false,
            creating: false,
            create_gen: 0,
            create_error: None,
        }
    }

    /// The selected machine row, if any.
    pub(crate) fn machine(&self) -> Option<&MachineRow> {
        self.machines.get(self.machine_sel)
    }

    /// The selected machine's `on:` name — `None` for Local.
    pub(crate) fn machine_name(&self) -> Option<String> {
        self.machine().and_then(|m| m.name.clone())
    }

    /// The selected machine's row label for titles and hints.
    pub(crate) fn machine_label(&self) -> String {
        self.machine()
            .map(|m| m.label.clone())
            .unwrap_or_else(|| "Local".into())
    }

    /// The chosen repository — the visible entry at the selection.
    pub(crate) fn repo(&self) -> Option<&RepoEntry> {
        self.repos.visible().get(self.repos.sel).copied()
    }

    /// `⏎`/`space` on a MODEL cell: the row's runner catalog as a
    /// list — `auto` first, the current value selected.
    pub(crate) fn open_model_pick(&mut self) {
        if let Probe::Done(runners) = &self.runners
            && let Some(row) = self.cast.rows.get(self.cast.sel)
            && let Some(runner) = runners.get(row.runner)
        {
            let current = runner.models.get(row.model).cloned().unwrap_or_default();
            self.model_pick = Some(ModelPick::open(&runner.models, &current));
        }
    }

    /// Seat `model` on the selected row — the `e` free-text rule:
    /// a name the catalog lacks joins it, the row points at it.
    pub(crate) fn seat_model(&mut self, model: &str) {
        if let Probe::Done(runners) = &mut self.runners
            && let Some(runner_index) = self.cast.rows.get(self.cast.sel).map(|r| r.runner)
            && let Some(runner) = runners.get_mut(runner_index)
        {
            let index = runner
                .models
                .iter()
                .position(|m| m == model)
                .unwrap_or_else(|| {
                    runner.models.push(model.to_string());
                    runner.models.len() - 1
                });
            if let Some(row) = self.cast.rows.get_mut(self.cast.sel) {
                row.model = index;
            }
        }
    }

    /// `Enter`'s gate on step 1: the machine is usable and a row is
    /// selected — every listed path binds (D4): an empty repository
    /// gets its initial commit at launch, a directory with member
    /// repositories is a workspace, a plain one gets `git init` or
    /// the step-4 no-git toggle; a studio-bound repo still warns in
    /// its note but passes.
    pub(crate) fn step1_enter_error(&self) -> Option<String> {
        let machine = self.machine()?;
        if !machine.usable() {
            return Some(match &machine.state {
                MachineState::Offline(why) => why.clone(),
                MachineState::Online => match &machine.backlot {
                    Probe::Pending => "checking for backlot…".into(),
                    Probe::Failed(e) => e.clone(),
                    Probe::Done(_) => unreachable!("usable checked above"),
                },
            });
        }
        if self.repos.listing.is_pending() {
            return Some("listing repositories…".into());
        }
        match self.repo() {
            Some(_) => None,
            None => Some("no path selected".into()),
        }
    }
}

/// The dialog's outer rect: a centered frame taking most of the
/// screen — capped so very wide terminals keep margins, and near
/// full-frame at 80×24 like the mockups.
pub(crate) fn frame(area: Rect) -> Option<Rect> {
    let w = area.width.saturating_sub(2).min(96);
    let h = (area.height.saturating_mul(5) / 6)
        .min(area.height.saturating_sub(2))
        .min(30);
    if w < 24 || h < 10 {
        return None;
    }
    Some(Rect::new(
        area.x + (area.width - w) / 2,
        area.y + (area.height - h) / 2,
        w,
        h,
    ))
}

/// The frame, title row, step tabs and hint row every step shares.
/// Returns the content rect the step draws into and the hits the
/// tabs/edges registered.
fn chrome(
    b: &mut Buffer,
    outer: Rect,
    d: &NewStudio,
    p: &Palette,
    hints: &mut NsHits,
    hint_tiers: &[&str],
) -> Option<Rect> {
    let inner = crate::chrome::dialogs::panel(b, outer, p.accent, p.panel_bg)?;
    hints.area = outer;
    // Title: `New studio · [n/4] Step Name` left, `esc …` right.
    // The title is clipped short of the right hint so they never
    // collide at narrow widths.
    let esc = match d.step {
        Step::MachineRepo => "esc cancel",
        _ => "esc step back",
    };
    let title_w = inner.width.saturating_sub(display_width(esc) as u16 + 1);
    let title = format!(" New studio · [{}/4] {} ", d.step.index(), d.step.label());
    put_text(
        b,
        inner.x,
        inner.y,
        title_w,
        &crate::chrome::text::truncate_end(&title, title_w as usize),
        Style::default()
            .fg(p.accent)
            .bg(p.panel_bg)
            .add_modifier(Modifier::BOLD),
    );
    put_right_text(
        b,
        inner,
        inner.y,
        esc,
        Style::default().fg(p.overlay0).bg(p.panel_bg),
    );
    // Step tabs on the second row — the current one highlighted. When
    // the four tabs cannot all fit the strip collapses to the compact
    // `n/4 <label>` form so the current step is always readable.
    let steps = [
        Step::MachineRepo,
        Step::IdTitle,
        Step::Cast,
        Step::CheckStart,
    ];
    let tab_w: u16 = steps
        .iter()
        .map(|step| display_width(&format!(" {} {} ", step.index(), step.label())) as u16 + 1)
        .sum();
    let y = inner.y + 1;
    if tab_w <= inner.width + 1 {
        let mut x = inner.x;
        for step in steps {
            let label = format!(" {} {} ", step.index(), step.label());
            let w = display_width(&label) as u16;
            let style = if step == d.step {
                Style::default()
                    .fg(crate::chrome::dialogs::contrast(p))
                    .bg(p.accent)
                    .add_modifier(Modifier::BOLD)
            } else if step.index() < d.step.index() {
                Style::default().fg(p.subtext0).bg(p.panel_bg)
            } else {
                Style::default().fg(p.overlay0).bg(p.panel_bg)
            };
            put_text(
                b,
                x,
                y,
                w.min(inner.right().saturating_sub(x)),
                &label,
                style,
            );
            if step.index() < d.step.index() {
                hints.push(Rect::new(x, y, w, 1), NsHit::StepTab(step));
            }
            x += w + 1;
        }
    } else {
        let compact = format!(" {}/4 {} ", d.step.index(), d.step.label());
        put_text(
            b,
            inner.x,
            y,
            inner.width,
            &crate::chrome::text::truncate_end(&compact, inner.width as usize),
            Style::default()
                .fg(crate::chrome::dialogs::contrast(p))
                .bg(p.accent)
                .add_modifier(Modifier::BOLD),
        );
    }
    // Bottom hint row — `fit_hints` picks the longest tier that fits.
    let hint_y = inner.bottom().saturating_sub(1);
    let hint = fit_hints(hint_tiers, usize::from(inner.width));
    put_text(
        b,
        inner.x,
        hint_y,
        inner.width,
        hint,
        Style::default().fg(p.overlay0).bg(p.panel_bg),
    );
    Some(Rect::new(
        inner.x,
        inner.y + 2,
        inner.width,
        inner.height.saturating_sub(3),
    ))
}

/// The selected-row marker and background — the panel's own
/// `▸` row convention.
fn selected_row(b: &mut Buffer, rect: Rect, p: &Palette) {
    b.set_style(rect, Style::default().bg(p.surface0));
}

/// The bg a row's text cells take: the selection surface when the
/// row is highlighted, else the panel — the selection must cover
/// the text, not just the padding around it.
fn row_text_bg(hot: bool, p: &Palette) -> ratatui::style::Color {
    if hot { p.surface0 } else { p.panel_bg }
}

/// The dialog's draw: dimmed backdrop is applied by the caller, the
/// frame is the overlay's own inside `band` (the modal band — the
/// whole frame minus a beside dock, so the dialog never crosses it),
/// then the step's body.
pub(crate) fn render(b: &mut Buffer, d: &NewStudio, p: &Palette, band: Rect) -> NsHits {
    let mut hits = NsHits::default();
    let Some(outer) = frame(band) else {
        // Too small to draw the frame — nothing but a one-line cry.
        put_text(
            b,
            b.area.x,
            b.area.y,
            b.area.width,
            "new studio: terminal too small",
            Style::default().fg(p.red),
        );
        return hits;
    };
    let tiers: &[&str] = match d.step {
        Step::MachineRepo => &[
            "tab pane  ↑↓ move  →/← dir  type to filter  ⏎ pick  ^R re-check  esc cancel",
            "tab pane  ↑↓  →/←  type to filter  ⏎ pick  esc",
            "↑↓  ⏎  esc",
            "esc",
        ],
        Step::IdTitle => &[
            "tab/↑↓ next field  ←→ mode  ⏎ continue to cast  esc back to repo",
            "tab field  ←→ mode  ⏎ continue  esc back",
            "⏎  esc",
        ],
        Step::Cast if d.model_pick.is_some() => {
            &["↑↓ move  type to filter  ⏎ pick  esc cancel", "↑↓  ⏎  esc"]
        }
        Step::Cast => &[
            "↑↓ role  ←→ value  ⏎ model list or dry-run  a add  x remove  ^E editor  esc back",
            "↑↓  ←→  ⏎ list or dry-run  a/x  ^E  esc",
            "↑↓  ⏎  esc",
        ],
        Step::CheckStart => &[
            "space toggle start-stage  ⏎ create studio  esc back to cast",
            "space toggle  ⏎ create  esc",
            "⏎  esc",
        ],
    };
    let Some(body) = chrome(b, outer, d, p, &mut hits, tiers) else {
        return hits;
    };
    match d.step {
        Step::MachineRepo => step1::draw(b, body, d, p, &mut hits),
        Step::IdTitle => step2::draw(b, body, d, p, &mut hits),
        Step::Cast => step3::draw(b, body, d, p, &mut hits),
        Step::CheckStart => step4::draw(b, body, d, p, &mut hits),
    }
    hits
}
