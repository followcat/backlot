//! The lifecycle dialogs (W4b): restart's two-choice reload, the
//! recast picker (runner → model → effort), and the archived-studios
//! restore list. Pure data and rules — the app owns opening, keys and
//! the submits, the renderers draw these fields.

use backlot_model::caps::{self, Effort};

use crate::chrome::text_editor::TextEditor;
use crate::new_studio::{Probe, RunnerRow};
use crate::studio::StudioKey;
use crate::studio_index::ArchivedStudio;

/// `Restart director · acme-shop · Local` — reload the seat in place,
/// resuming its recorded session or starting fresh.
#[derive(Debug, Clone)]
pub(crate) struct RestartDialog {
    pub(crate) title: String,
    /// `None` for Hollywood, whose restart lives in its own ledger.
    pub(crate) key: Option<StudioKey>,
    /// The ledger key `restart --person` takes.
    pub(crate) role: String,
    /// The runner resumes and a session is likely recorded — the
    /// first choice is selectable and preselected.
    pub(crate) resumable: bool,
    /// 0 `Resume its session`, 1 `Fresh start`.
    pub(crate) selected: usize,
}

/// One restart choice: what the dialog draws and what Enter runs.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct RestartChoice {
    pub(crate) label: &'static str,
    pub(crate) fresh: bool,
    /// `Some(reason)` dims the row and makes a press do nothing.
    pub(crate) dim: Option<&'static str>,
}

impl RestartDialog {
    pub(crate) fn new(
        key: Option<StudioKey>,
        role: &str,
        role_name: &str,
        resumable: bool,
    ) -> RestartDialog {
        let place = key
            .as_ref()
            .map(|k| format!(" · {}", crate::studio_dialog::StudioDialog::place(k)))
            .unwrap_or_default();
        RestartDialog {
            title: format!("Restart {role_name}{place}"),
            key,
            role: role.to_owned(),
            resumable,
            selected: if resumable { 0 } else { 1 },
        }
    }

    /// Both choices, every time; `Resume` dims when the runner keeps no
    /// resumable session.
    pub(crate) fn choices(&self) -> Vec<RestartChoice> {
        vec![
            RestartChoice {
                label: "Resume its session",
                fresh: false,
                dim: (!self.resumable).then_some("no saved session"),
            },
            RestartChoice {
                label: "Fresh start",
                fresh: true,
                dim: None,
            },
        ]
    }

    /// The selected choice's argv flag — `None` when the row is dim.
    pub(crate) fn chosen(&self) -> Option<RestartChoice> {
        self.choices()
            .get(self.selected)
            .copied()
            .filter(|choice| choice.dim.is_none())
    }

    pub(crate) fn move_selection(&mut self, delta: isize) {
        let n = self.choices().len() as isize;
        self.selected = (self.selected as isize + delta).rem_euclid(n) as usize;
    }
}

/// Where the recast picker stands.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum RecastStep {
    Runner,
    Model,
    Effort,
    /// The summary line: Enter applies, `←` steps back.
    Apply,
}

/// `Recast director · acme-shop · Local` — pick runner, model, then
/// the effort its caps allow, and apply.
#[derive(Debug, Clone)]
pub(crate) struct RecastDialog {
    pub(crate) title: String,
    pub(crate) key: StudioKey,
    pub(crate) role: String,
    pub(crate) role_name: String,
    pub(crate) current_runner: Option<String>,
    pub(crate) current_model: Option<String>,
    pub(crate) current_effort: Option<String>,
    /// The machine's `runners --json` answer.
    pub(crate) runners: Probe<Vec<RunnerRow>>,
    pub(crate) step: RecastStep,
    pub(crate) runner_sel: usize,
    pub(crate) model_sel: usize,
    pub(crate) effort_sel: usize,
    pub(crate) error: Option<String>,
}

/// One effort line: its label and what `--effort` receives (`None` is
/// the allowed empty choice).
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct EffortChoice {
    pub(crate) label: String,
    pub(crate) value: Option<String>,
}

impl RecastDialog {
    pub(crate) fn new(
        key: &StudioKey,
        role: &str,
        role_name: &str,
        runner: Option<String>,
        model: Option<String>,
        effort: Option<String>,
    ) -> RecastDialog {
        RecastDialog {
            title: format!(
                "Recast {role_name} · {}",
                crate::studio_dialog::StudioDialog::place(key)
            ),
            key: key.clone(),
            role: role.to_owned(),
            role_name: role_name.to_owned(),
            current_runner: runner,
            current_model: model,
            current_effort: effort,
            runners: Probe::Pending,
            step: RecastStep::Runner,
            runner_sel: 0,
            model_sel: 0,
            effort_sel: 0,
            error: None,
        }
    }

    /// The runners that can seat a role, detection's own `found` rule.
    pub(crate) fn found(&self) -> Vec<&RunnerRow> {
        match &self.runners {
            Probe::Done(rows) => rows.iter().filter(|r| r.found()).collect(),
            _ => Vec::new(),
        }
    }

    pub(crate) fn selected_runner(&self) -> Option<&RunnerRow> {
        self.found().get(self.runner_sel).copied()
    }

    pub(crate) fn runner_name(&self) -> Option<&str> {
        self.selected_runner().map(|r| r.runner.as_str())
    }

    pub(crate) fn models(&self) -> Vec<String> {
        self.selected_runner()
            .map(|r| r.models.clone())
            .unwrap_or_default()
    }

    pub(crate) fn selected_model(&self) -> Option<String> {
        self.models().get(self.model_sel).cloned()
    }

    /// The effort choices the selected runner's caps allow: its levels
    /// (required), an empty choice plus any levels (optional), or
    /// nothing at all (no effort flag).
    pub(crate) fn efforts(&self) -> Vec<EffortChoice> {
        let Some(runner) = self.runner_name() else {
            return Vec::new();
        };
        let Some(caps) = caps::get(runner) else {
            return Vec::new();
        };
        match caps.effort {
            Effort::Required(levels) => levels
                .iter()
                .map(|level| EffortChoice {
                    label: (*level).to_owned(),
                    value: Some((*level).to_owned()),
                })
                .collect(),
            Effort::Optional(levels) => std::iter::once(EffortChoice {
                label: "(none)".to_owned(),
                value: None,
            })
            .chain(levels.iter().map(|level| EffortChoice {
                label: (*level).to_owned(),
                value: Some((*level).to_owned()),
            }))
            .collect(),
            Effort::None => Vec::new(),
        }
    }

    pub(crate) fn selected_effort(&self) -> Option<String> {
        self.efforts()
            .get(self.effort_sel)
            .and_then(|e| e.value.clone())
    }

    /// Whether the runner has an effort row at all.
    pub(crate) fn has_effort(&self) -> bool {
        !self.efforts().is_empty()
    }

    /// Reseat the picker on the selected runner: models keep the
    /// current one when its runner matches, efforts start on the
    /// runner's default (or the current level when it is one).
    pub(crate) fn reseat(&mut self) {
        let current_runner = self.current_runner.as_deref();
        let runner = self.runner_name().map(str::to_owned);
        let same_runner = runner.as_deref() == current_runner;
        let models = self.models();
        self.model_sel = if same_runner {
            self.current_model
                .as_deref()
                .and_then(|model| models.iter().position(|m| m == model))
                .unwrap_or(0)
        } else {
            0
        };
        let efforts = self.efforts();
        self.effort_sel = if efforts.is_empty() {
            0
        } else if same_runner && let Some(current) = self.current_effort.as_deref() {
            efforts
                .iter()
                .position(|e| e.value.as_deref() == Some(current))
                .unwrap_or_else(|| default_effort(&efforts))
        } else {
            default_effort(&efforts)
        };
    }

    /// The step after the current one — a no-effort runner skips its
    /// row entirely.
    pub(crate) fn next_step(&self) -> RecastStep {
        match self.step {
            RecastStep::Runner | RecastStep::Model => {
                if self.has_effort() {
                    RecastStep::Effort
                } else {
                    RecastStep::Apply
                }
            }
            RecastStep::Effort => RecastStep::Apply,
            RecastStep::Apply => RecastStep::Apply,
        }
    }

    /// `director: claude/claude-sonnet-5-5/high → cursor/auto` — the
    /// one line that says what changes.
    pub(crate) fn summary(&self) -> String {
        let before = match (&self.current_runner, &self.current_model) {
            (Some(r), Some(m)) => match &self.current_effort {
                Some(e) => format!("{r}/{m}/{e}"),
                None => format!("{r}/{m}"),
            },
            _ => "(not recorded)".to_owned(),
        };
        let runner = self.runner_name().unwrap_or("-");
        let model = self.selected_model().unwrap_or_default();
        let after = match self.selected_effort() {
            Some(effort) => format!("{runner}/{model}/{effort}"),
            None => format!("{runner}/{model}"),
        };
        format!("{}: {before} → {after}", self.role_name)
    }
}

/// The effort preselected for a runner: `medium` when it is a level,
/// else the first real level (an optional runner's empty choice is
/// never the automatic pick while levels exist).
fn default_effort(efforts: &[EffortChoice]) -> usize {
    efforts
        .iter()
        .position(|e| e.value.as_deref() == Some("medium"))
        .or_else(|| efforts.iter().position(|e| e.value.is_some()))
        .unwrap_or(0)
}

/// Where the restore dialog stands.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum RestoreStep {
    /// The archived studios list.
    List,
    /// `Restore as <editable name>`.
    Name,
    /// The `--dry-run` plan, with Enter restoring for real.
    Confirm,
}

/// The archived studios of one machine, and the restore's own steps.
#[derive(Debug, Clone)]
pub(crate) struct RestoreDialog {
    pub(crate) machine: Option<String>,
    pub(crate) entries: Vec<ArchivedStudio>,
    pub(crate) sel: usize,
    pub(crate) step: RestoreStep,
    /// The name the restored studio takes.
    pub(crate) name: TextEditor,
    /// The dry-run plan, verbatim.
    pub(crate) body: String,
    pub(crate) loading: bool,
    pub(crate) error: Option<String>,
}

impl RestoreDialog {
    pub(crate) fn new(machine: Option<String>, entries: Vec<ArchivedStudio>) -> RestoreDialog {
        RestoreDialog {
            machine,
            entries,
            sel: 0,
            step: RestoreStep::List,
            name: TextEditor::new("", false),
            body: String::new(),
            loading: false,
            error: None,
        }
    }

    pub(crate) fn selected(&self) -> Option<&ArchivedStudio> {
        self.entries.get(self.sel)
    }

    pub(crate) fn title(&self) -> String {
        let machine = self.machine.as_deref().unwrap_or("Local");
        match self.step {
            RestoreStep::List => format!("Archived studios · {machine}"),
            RestoreStep::Name | RestoreStep::Confirm => {
                let entry = self
                    .selected()
                    .map(|e| e.name.as_str())
                    .unwrap_or("archived");
                format!("Restore {entry} · {machine}")
            }
        }
    }

    /// The default name for the chosen entry: the archived studio id,
    /// else the entry name.
    pub(crate) fn default_name(&self) -> String {
        self.selected()
            .map(|e| e.studio.clone().unwrap_or_else(|| e.name.clone()))
            .unwrap_or_default()
    }

    pub(crate) fn move_selection(&mut self, delta: isize) {
        if self.entries.is_empty() {
            return;
        }
        let n = self.entries.len() as isize;
        self.sel = (self.sel as isize + delta).rem_euclid(n) as usize;
    }

    /// Step into `Restore as <name>` with the default name seeded.
    pub(crate) fn begin_name(&mut self) {
        self.name = TextEditor::new(&self.default_name(), false);
        self.error = None;
        self.body.clear();
        self.step = RestoreStep::Name;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn row(runner: &str, models: &[&str], found: bool) -> RunnerRow {
        RunnerRow {
            runner: runner.into(),
            status: if found { "found" } else { "missing" }.into(),
            version: None,
            path: None,
            needs_path: false,
            models: models.iter().map(|m| (*m).to_string()).collect(),
            efforts: Vec::new(),
        }
    }

    fn recast(runner: &str) -> RecastDialog {
        let mut d = RecastDialog::new(
            &StudioKey::local("acme-shop"),
            "director",
            "director",
            None,
            None,
            None,
        );
        d.runners = Probe::Done(vec![row(runner, &["m1", "m2"], true)]);
        d.reseat();
        d
    }

    #[test]
    fn a_required_effort_runner_lists_its_levels() {
        let d = recast("claude");
        let labels: Vec<String> = d.efforts().into_iter().map(|e| e.label).collect();
        assert_eq!(labels, ["low", "medium", "high", "xhigh", "max"]);
        assert_eq!(d.efforts()[d.effort_sel].label, "medium", "preselected");
    }

    #[test]
    fn an_optional_effort_runner_allows_empty() {
        let d = recast("devin");
        let choices = d.efforts();
        assert_eq!(choices.len(), 1, "no levels; the empty choice alone");
        assert_eq!(choices[0].value, None);
    }

    #[test]
    fn a_no_effort_runner_has_no_effort_row_and_skips_to_apply() {
        let mut d = recast("cursor");
        assert!(d.efforts().is_empty());
        d.step = RecastStep::Model;
        assert_eq!(d.next_step(), RecastStep::Apply);
    }

    #[test]
    fn the_summary_names_both_identities() {
        let mut d = recast("claude");
        d.current_runner = Some("claude".into());
        d.current_model = Some("m1".into());
        d.current_effort = Some("low".into());
        d.model_sel = 1;
        assert_eq!(d.summary(), "director: claude/m1/low → claude/m2/medium");
    }
}
