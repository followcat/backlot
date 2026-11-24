//! Effort-rule and summary tests for the recast picker.

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
