//! Unified Hollywood agent switch / setup picker (§2.2.2).
//! Replaces both `HwSetup` and the Hollywood branch of `RecastDialog`:
//! probes `backlot runners --json`, pre-fills the current identity (or
//! highlights an installed alternative on quota/error), refuses
//! unchanged identities, shows make-before-break reassurance, and
//! submits either `hollywood switch --no-wait` (when live) or initial
//! `hollywood setup` (when unconfigured/offline).

use ratatui::layout::Rect;

use backlot_stage::config::HollywoodCfg;

use crate::chrome::help::CursorState;
use crate::new_studio::{ModelPick, Probe, RunnerRow};

mod render;
#[cfg(test)]
pub(crate) use render::render;
pub(crate) use render::render_with_shadow;

#[cfg(test)]
#[path = "tests.rs"]
mod tests;

/// The dialog's interactive rows — `Effort` only exists while the
/// selected runner declares an effort list.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Field {
    Runner,
    Model,
    Effort,
    Apply,
}

/// Unified state for switching or initially configuring Hollywood's agent.
pub(crate) struct HwSwitch {
    pub runners: Probe<Vec<RunnerRow>>,
    pub runners_gen: u64,
    pub runner: usize,
    pub model: String,
    pub model_edit: Option<crate::chrome::text_editor::TextEditor>,
    pub model_pick: Option<ModelPick>,
    pub effort: usize,
    pub field: Field,
    pub existing: Option<HollywoodCfg>,
    pub prefer_alternative: bool,
    pub error: Option<String>,
    pub(crate) prefilled: bool,
}

/// One clickable cell in the last render.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum HwHit {
    Field(Field),
    ModelPick(usize),
}

/// The hit map the last render left.
#[derive(Default)]
pub(crate) struct HwHits {
    pub area: Rect,
    pub cells: Vec<(Rect, HwHit)>,
    pub cursor: Option<CursorState>,
}

impl HwHits {
    pub(super) fn push(&mut self, rect: Rect, hit: HwHit) {
        if rect.width > 0 && rect.height > 0 {
            self.cells.push((rect, hit));
        }
    }
}

impl HwSwitch {
    pub(crate) fn new(existing: Option<HollywoodCfg>, prefer_alternative: bool) -> Self {
        HwSwitch {
            runners: Probe::Pending,
            runners_gen: 0,
            runner: 0,
            model: "auto".to_string(),
            model_edit: None,
            model_pick: None,
            effort: 0,
            field: Field::Runner,
            existing,
            prefer_alternative,
            error: None,
            prefilled: false,
        }
    }

    /// Formatted `Current: runner / model / effort` header when an
    /// identity is already configured.
    pub(crate) fn current_label(&self) -> Option<String> {
        let old = self.existing.as_ref()?;
        let mut parts = vec![old.runner.as_str(), old.model.as_str()];
        if let Some(eff) = old.effort.as_deref().filter(|e| *e != "auto") {
            parts.push(eff);
        }
        Some(format!("Current: {}", parts.join(" / ")))
    }

    /// Seat the prefill on the first probe answer. When opened from a
    /// quota/error row (`prefer_alternative`), highlight another found
    /// runner if available; otherwise pre-select the current identity.
    pub(crate) fn prefill(&mut self) {
        if self.prefilled || !matches!(self.runners, Probe::Done(_)) {
            return;
        }
        self.prefilled = true;
        let Some(old) = &self.existing else {
            return;
        };
        let pick = {
            let found = self.found();
            if self.prefer_alternative
                && let Some(alt) = found.iter().position(|r| r.runner != old.runner)
            {
                Some((alt, String::from("auto"), None))
            } else {
                found.iter().position(|r| r.runner == old.runner).map(|i| {
                    (
                        i,
                        old.model.clone(),
                        old.effort
                            .as_ref()
                            .and_then(|e| found[i].efforts.iter().position(|v| v == e)),
                    )
                })
            }
        };
        if let Some((i, model, effort)) = pick {
            self.runner = i;
            self.model = model;
            self.effort = effort.unwrap_or(0);
        }
    }

    /// Whether the chosen identity matches `existing` unchanged.
    pub(crate) fn unchanged(&self) -> bool {
        let Some(old) = &self.existing else {
            return false;
        };
        let Some(row) = self.runner_row() else {
            return false;
        };
        if row.runner != old.runner || self.model != old.model {
            return false;
        }
        let norm_old = old.effort.as_deref().filter(|e| *e != "auto");
        let norm_new = self.effort_value().filter(|e| *e != "auto");
        norm_old == norm_new
    }

    /// Build the chosen `HollywoodCfg`, carrying over `key`/`default_cast`.
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

    pub(crate) fn found(&self) -> Vec<&RunnerRow> {
        match &self.runners {
            Probe::Done(rows) => rows.iter().filter(|r| r.found()).collect(),
            _ => Vec::new(),
        }
    }

    pub(crate) fn runner_row(&self) -> Option<&RunnerRow> {
        self.found().get(self.runner).copied()
    }

    fn efforts(&self) -> &[String] {
        self.runner_row()
            .map(|r| r.efforts.as_slice())
            .unwrap_or(&[])
    }

    fn has_effort(&self) -> bool {
        !self.efforts().is_empty()
    }

    pub(crate) fn fields(&self) -> Vec<Field> {
        let mut out = vec![Field::Runner, Field::Model];
        if self.has_effort() {
            out.push(Field::Effort);
        }
        out.push(Field::Apply);
        out
    }

    pub(crate) fn move_field(&mut self, delta: isize) {
        let fields = self.fields();
        let index = fields.iter().position(|f| *f == self.field).unwrap_or(0) as isize;
        let next = (index + delta).rem_euclid(fields.len() as isize) as usize;
        self.field = fields[next];
    }

    pub(crate) fn cycle(&mut self, delta: isize) {
        self.error = None;
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

    pub(crate) fn open_pick(&mut self) {
        self.error = None;
        if let Some(runner) = self.runner_row() {
            self.model_pick = Some(ModelPick::open(&runner.models, &self.model));
        }
    }

    pub(crate) fn effort_value(&self) -> Option<&str> {
        self.efforts().get(self.effort).map(String::as_str)
    }
}
