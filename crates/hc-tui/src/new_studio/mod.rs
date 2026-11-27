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

mod chrome;
mod model;
mod step1;
mod step2;
mod step3;
mod step4;

#[cfg(test)]
#[path = "tests.rs"]
mod tests;

pub(crate) use model::{
    CastTable, MachineRow, MachineState, ModelPick, PathShape, Probe, RepoEntry, RepoKind, Repos,
    RunnerRow, Step, Step1Pane, id_error, parse_repos, parse_runners, studio_id, title_prefill,
};

use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::Style;

use crate::chrome::help::CursorState;
use crate::chrome::palette::Palette;
use crate::chrome::render::put_text;
use crate::chrome::text_editor::TextEditor;

/// Step 2's focusable fields — `Mode` is a two-value toggle, not
/// a text field: `←`/`→`/space flips it. `Build` is the optional
/// multi-line "what should this studio build?" brief.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Step2Field {
    Id,
    Title,
    Mode,
    Build,
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
    /// The optional "what should this studio build?" brief, edited on
    /// step 2 — multi-line: `alt+⏎`, paste or `^E` add lines. Empty
    /// means today's behaviour: the producer asks first.
    pub build: crate::studio_dialog::BodyEditor,
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
    /// The step-2 "what should this studio build?" body rows.
    BuildField,
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
            build: crate::studio_dialog::BodyEditor::default(),
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

    /// The build brief as text — trimmed; empty means "ask as
    /// before". Only whitespace counts as empty.
    pub(crate) fn build_text(&self) -> String {
        self.build.text().trim().to_string()
    }

    /// The build brief's first non-empty line — step 4's summary
    /// card shows it, so a long brief stays one readable row.
    pub(crate) fn build_summary(&self) -> Option<String> {
        self.build
            .text()
            .lines()
            .map(str::trim)
            .find(|l| !l.is_empty())
            .map(str::to_string)
    }
}

/// The dialog's outer rect: a centered frame taking most of the
/// screen — capped so very wide terminals keep margins, and near
/// full-frame at 80×24 like the mockups.
pub(super) fn frame_content_size(area: Rect) -> Option<(u16, u16)> {
    chrome::frame_content_size(area)
}

/// The selected-row marker and background — the panel's own
/// `▸` row convention.
fn selected_row(b: &mut Buffer, rect: Rect, p: &Palette) {
    b.set_style(rect, Style::default().bg(p.elevation().raised_on_overlay));
}

/// The bg a row's text cells take: the selection surface when the
/// row is highlighted, else the panel — the selection must cover
/// the text, not just the padding around it.
fn row_text_bg(hot: bool, p: &Palette) -> ratatui::style::Color {
    if hot {
        p.elevation().raised_on_overlay
    } else {
        crate::chrome::popup_frame::overlay_bg(p)
    }
}

/// The dialog's draw: dimmed backdrop is applied by the caller, the
/// frame is the overlay's own inside `band` (the modal band — the
/// whole frame minus a beside dock, so the dialog never crosses it),
/// then the step's body.
#[cfg_attr(not(test), allow(dead_code))]
pub(crate) fn render(b: &mut Buffer, d: &NewStudio, p: &Palette, band: Rect) -> NsHits {
    render_with_shadow(b, d, p, band, true)
}

pub(crate) fn render_with_shadow(
    b: &mut Buffer,
    d: &NewStudio,
    p: &Palette,
    band: Rect,
    shadow: bool,
) -> NsHits {
    render_styled(b, d, p, band, shadow, true)
}

pub(crate) fn render_styled(
    b: &mut Buffer,
    d: &NewStudio,
    p: &Palette,
    band: Rect,
    shadow: bool,
    deco: bool,
) -> NsHits {
    let mut hits = NsHits::default();
    if frame_content_size(band).is_none() {
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
    }
    let tiers: &[&str] = match d.step {
        Step::MachineRepo => &[
            "tab pane  ↑↓ move  →/← dir  type to filter  ⏎ pick  ^R re-check  esc cancel",
            "tab pane  ↑↓  →/←  type to filter  ⏎ pick  esc",
            "↑↓  ⏎  esc",
            "esc",
        ],
        Step::IdTitle => &[
            "tab/↑↓ next field  ←→ mode  alt+⏎ line  ^E editor  ⏎ continue  esc back",
            "tab field  ←→ mode  ^E editor  ⏎ continue  esc back",
            "tab  ⏎  esc",
        ],
        Step::Cast if d.model_pick.is_some() => {
            &["↑↓ move  type to filter  ⏎ pick  esc cancel", "↑↓  ⏎  esc"]
        }
        // `⏎` is cell-dependent — the MODEL cell opens its runner's
        // catalog, the others dry-run — so the footer names the
        // focused cell's meaning and teaches `tab`.
        Step::Cast => step3::hint_tiers(d.cast.col),
        Step::CheckStart => &[
            "space toggle start-stage  ⏎ create studio  esc back to cast",
            "space toggle  ⏎ create  esc",
            "⏎  esc",
        ],
    };
    let Some(body) = chrome::draw_chrome(
        b,
        band,
        d,
        p,
        &mut hits,
        tiers,
        chrome::ChromeStyle { shadow, deco },
    ) else {
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
