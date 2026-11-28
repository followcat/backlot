//! Agent Skill sync in the client: the machine-row indicator, the
//! one-key notice and its per-machine/version dismissal, the Settings
//! tree and the machine menu entry. Neutral fixtures only.

use super::endpoint_nav::remote;
use super::studio_machines::two_machine_studio_app;
use super::*;
use crate::chrome::render::MachineSkillMark;
use crate::chrome::settings::{SettingsSection, SkillMachineView, SkillSettingsRow, skill_rows};
use crate::test_support::TestDir;
use hc_proto::watch::DocKey;

fn key(machine: Option<&str>) -> DocKey {
    DocKey {
        machine: machine.map(str::to_owned),
        name: "backlot".into(),
    }
}

fn publish_skill(app: &mut App, machine: Option<&str>, version: &str, rows: serde_json::Value) {
    let doc = app
        .view
        .documents_mut()
        .get_mut(&key(machine))
        .expect("machine backlot document");
    std::sync::Arc::make_mut(doc).value["backlot_version"] =
        serde_json::Value::String(version.into());
    std::sync::Arc::make_mut(doc).value["agent_skill"] = serde_json::json!({
        "version": version,
        "rows": rows,
    });
    app.reconcile_published_view();
}

fn rows(states: &[(&str, &str)]) -> serde_json::Value {
    serde_json::Value::Array(
        states
            .iter()
            .map(|(agent, state)| {
                serde_json::json!({
                    "agent": agent,
                    "dir": "~/.agents/skills",
                    "state": state,
                })
            })
            .collect(),
    )
}

fn notice_text(app: &App) -> String {
    app.notice
        .as_ref()
        .map(|notice| format!("{}\n{}", notice.title, notice.body))
        .unwrap_or_default()
}

#[tokio::test]
async fn the_machine_row_marks_missing_stale_current_and_unknown() {
    let mut app = two_machine_studio_app();
    publish_skill(
        &mut app,
        Some("host-b"),
        "0.4.32",
        rows(&[("codex", "missing")]),
    );
    publish_skill(&mut app, None, "0.4.32", rows(&[("codex", "stale")]));
    let lines = app.machine_lines();
    assert_eq!(
        lines[&remote("host-b")].skill,
        Some(MachineSkillMark::Missing)
    );
    assert_eq!(
        lines[&crate::chrome::snapshot::ClientEndpointId::Local].skill,
        Some(MachineSkillMark::Stale)
    );

    publish_skill(
        &mut app,
        Some("host-b"),
        "0.4.32",
        rows(&[("codex", "current")]),
    );
    assert_eq!(
        app.machine_lines()[&remote("host-b")].skill,
        Some(MachineSkillMark::Current)
    );
    app.view
        .documents_mut()
        .get_mut(&key(Some("host-b")))
        .and_then(|doc| std::sync::Arc::make_mut(doc).value.as_object_mut())
        .unwrap()
        .remove("agent_skill");
    app.reconcile_published_view();
    assert_eq!(
        app.machine_lines()[&remote("host-b")].skill,
        Some(MachineSkillMark::Unknown)
    );
}

#[tokio::test]
async fn the_notice_appears_for_missing_or_stale_but_not_foreign_or_absent() {
    let mut app = two_machine_studio_app();
    publish_skill(
        &mut app,
        Some("host-b"),
        "0.4.32",
        rows(&[("codex", "missing"), ("devin", "stale")]),
    );
    app.sync_skill_notice();
    let text = notice_text(&app);
    assert!(
        text.contains("Agent skill missing and out of date on host-b (codex, devin)"),
        "{text}"
    );
    assert!(
        text.contains(" ↵ install") || text.contains("\n↵ install"),
        "{text}"
    );
    assert!(text.contains("d dismiss"), "{text}");

    let mut foreign = two_machine_studio_app();
    publish_skill(
        &mut foreign,
        Some("host-b"),
        "0.4.32",
        rows(&[("codex", "foreign"), ("devin", "absent")]),
    );
    foreign.sync_skill_notice();
    assert_eq!(notice_text(&foreign), "");
}

#[tokio::test]
async fn dismissal_is_per_machine_and_backlot_version() {
    let dir = TestDir::new("hctui-skill-dismiss");
    let mut app = two_machine_studio_app();
    app.update_state_path = Some(dir.join("update-check.json"));
    publish_skill(
        &mut app,
        Some("host-b"),
        "0.4.32",
        rows(&[("codex", "missing")]),
    );
    app.sync_skill_notice();
    assert!(app.skill_notice.is_some());
    app.skill_notice_dismiss();
    assert!(app.skill_notice.is_none());
    assert!(dir.join("skill-dismissals.json").exists());

    app.sync_skill_notice();
    assert!(
        app.skill_notice.is_none(),
        "same machine/version stays hidden"
    );
    publish_skill(
        &mut app,
        Some("host-b"),
        "0.4.33",
        rows(&[("codex", "missing")]),
    );
    app.sync_skill_notice();
    assert!(
        app.skill_notice.is_some(),
        "a version change shows it again"
    );
}

#[tokio::test]
async fn settings_lists_the_filtered_machines_agent_rows() {
    let mut app = two_machine_studio_app();
    publish_skill(
        &mut app,
        Some("host-b"),
        "0.4.32",
        rows(&[("codex", "missing"), ("devin", "foreign")]),
    );
    app.open_agent_skill_settings(Some("host-b".into()));
    let settings = app.settings.as_ref().expect("settings");
    assert_eq!(settings.section, SettingsSection::AgentSkill);
    let text = screen_lines(&draw_to(&mut app, 120, 36)).join("\n");
    assert!(text.contains("agent skill"), "{text}");
    assert!(text.contains("host-b"), "{text}");
    assert!(text.contains("codex"), "{text}");
    assert!(text.contains("missing"), "{text}");
    assert!(!text.contains("Local —"), "{text}");
}

#[tokio::test]
async fn settings_orders_rows_shows_unknown_machines_and_never_clips_hints() {
    let mut app = two_machine_studio_app();
    publish_skill(
        &mut app,
        Some("host-b"),
        "0.4.32",
        serde_json::json!([
            {"agent":"opencode","dir":"~/.config/opencode/skills","state":"absent"},
            {"agent":"devin","dir":"~/.config/devin/skills","state":"foreign"},
            {"agent":"codex","dir":"~/.agents/skills","state":"missing"},
            {"agent":"cursor","dir":"~/.cursor/skills","state":"stale"},
            {"agent":"cline","dir":"~/.agents/skills","state":"current"}
        ]),
    );
    app.open_agent_skill_settings(Some("host-b".into()));
    let states: Vec<_> = app
        .settings
        .as_ref()
        .and_then(|settings| {
            settings
                .agent_skill
                .iter()
                .find(|machine| machine.machine.as_deref() == Some("host-b"))
        })
        .map(|machine| machine.rows.iter().map(|row| row.state.as_str()).collect())
        .unwrap();
    assert_eq!(states, ["current", "stale", "missing", "foreign", "absent"]);

    let backend = draw_to(&mut app, 80, 36);
    let text = screen_lines(&backend).join("\n");
    assert!(text.contains("host-b — install 2 agents"), "{text}");
    assert!(text.contains("not detected: opencode"), "{text}");
    assert!(!text.contains("~/.config/opencode/skills"), "{text}");
    assert_eq!(text.matches("enter install").count(), 1, "{text}");
    assert_eq!(text.matches("u uninstall").count(), 1, "{text}");
    assert!(!text.contains("u unin…"), "{text}");
    assert!(!text.contains("enter replace…"), "{text}");
    let lines = screen_lines(&backend);
    let absent_y = lines
        .iter()
        .position(|line| line.contains("opencode"))
        .expect("absent row");
    let absent_x = lines[absent_y].find("opencode").expect("absent text") as u16;
    assert_eq!(
        backend.buffer()[(absent_x, absent_y as u16)].fg,
        app.shell.palette.overlay0,
        "absent rows are dim"
    );

    let row_names: Vec<String> = skill_rows(app.settings.as_ref().unwrap())
        .into_iter()
        .filter_map(|row| match row {
            SkillSettingsRow::Agent { agent, .. } => Some(agent),
            _ => None,
        })
        .collect();
    assert_eq!(row_names, ["cline", "cursor", "codex", "devin"]);
}

#[tokio::test]
async fn settings_lists_a_connected_machine_with_no_published_skill_state() {
    let mut app = two_machine_studio_app();
    app.open_agent_skill_settings(Some("host-b".into()));
    let machine = app
        .settings
        .as_ref()
        .and_then(|settings| {
            settings
                .agent_skill
                .iter()
                .find(|machine| machine.machine.as_deref() == Some("host-b"))
        })
        .expect("host-b is a connected machine");
    assert!(!machine.known);
    let text = screen_lines(&draw_to(&mut app, 80, 36)).join("\n");
    assert!(
        text.contains("host-b — unknown (its stage publishes no skill state)"),
        "{text}"
    );
    assert!(!text.contains("host-b — install"), "{text}");
}

#[tokio::test]
async fn settings_at_120x36_folds_absent_rows_shows_every_machine_and_scrolls() {
    let mut app = two_machine_studio_app();
    publish_skill(
        &mut app,
        None,
        "0.4.33",
        rows(&[
            ("cursor", "missing"),
            ("claude-code", "absent"),
            ("codex", "absent"),
            ("cline", "absent"),
            ("pi", "absent"),
        ]),
    );
    publish_skill(
        &mut app,
        Some("host-b"),
        "0.4.33",
        rows(&[("codex", "current"), ("devin", "absent")]),
    );
    app.view
        .machines
        .as_mut()
        .expect("machine list")
        .push(crate::sidebar::fixture::machine("host-c", "connected"));
    app.reconcile_published_view();

    app.open_settings_overlay();
    app.select_settings_section(SettingsSection::AgentSkill);
    let text = screen_lines(&draw_to(&mut app, 120, 36)).join("\n");
    assert!(text.contains("Local — install 1 agent"), "{text}");
    assert!(text.contains("host-b — current"), "{text}");
    assert!(
        text.contains("host-c — unknown (its stage publishes no skill state)"),
        "{text}"
    );
    assert!(
        text.contains("not detected: claude-code, codex, cline, pi"),
        "{text}"
    );
    assert!(!text.contains("~/.claude/skills"), "{text}");

    if let Some(settings) = app.settings.as_mut() {
        settings.agent_skill = (0..30)
            .map(|index| {
                let name = format!("host-{index}");
                SkillMachineView {
                    machine: Some(name.clone()),
                    label: name,
                    rows: Vec::new(),
                    known: true,
                }
            })
            .collect();
        settings.selected = 0;
    }
    let text = screen_lines(&draw_to(&mut app, 120, 36)).join("\n");
    assert!(text.contains("↓ more"), "{text}");
    let last = skill_rows(app.settings.as_ref().unwrap()).len() - 1;
    app.settings.as_mut().unwrap().selected = last;
    let text = screen_lines(&draw_to(&mut app, 120, 36)).join("\n");
    assert!(text.contains("host-29 — current"), "{text}");
}

#[tokio::test]
async fn settings_pluralizes_machine_install_rows() {
    let mut app = two_machine_studio_app();
    publish_skill(
        &mut app,
        Some("host-b"),
        "0.4.32",
        rows(&[("codex", "missing"), ("devin", "missing")]),
    );
    app.open_agent_skill_settings(Some("host-b".into()));
    let text = screen_lines(&draw_to(&mut app, 80, 36)).join("\n");
    assert!(text.contains("host-b — install 2 agents"), "{text}");
    assert!(!text.contains("install 2 agent rows"), "{text}");

    publish_skill(
        &mut app,
        Some("host-b"),
        "0.4.32",
        rows(&[("codex", "missing")]),
    );
    app.open_agent_skill_settings(Some("host-b".into()));
    let text = screen_lines(&draw_to(&mut app, 80, 36)).join("\n");
    assert!(text.contains("host-b — install 1 agent"), "{text}");
    assert!(!text.contains("install 1 agents"), "{text}");
}

#[tokio::test]
async fn the_machine_menu_opens_agent_skill_settings() {
    let mut app = two_machine_studio_app();
    publish_skill(
        &mut app,
        Some("host-b"),
        "0.4.32",
        rows(&[("codex", "current")]),
    );
    let menu = app.machine_menu(&remote("host-b"));
    let labels = menu
        .items()
        .into_iter()
        .map(|item| item.label)
        .collect::<Vec<_>>();
    assert!(labels.contains(&"Agent skill…".to_owned()), "{labels:?}");
    let index = labels
        .iter()
        .position(|label| label == "Agent skill…")
        .unwrap();
    app.run_studio_act(&menu, menu.items()[index].act);
    let settings = app.settings.as_ref().expect("settings");
    assert_eq!(settings.section, SettingsSection::AgentSkill);
    assert_eq!(
        settings.agent_skill_filter,
        Some(Some("host-b".into())),
        "the machine menu filters the view"
    );
}
