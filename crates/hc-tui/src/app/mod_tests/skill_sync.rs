//! Agent Skill sync in the client: the machine-row indicator, the
//! one-key notice and its per-machine/version dismissal, the Settings
//! tree and the machine menu entry. Neutral fixtures only.

use super::endpoint_nav::remote;
use super::studio_machines::two_machine_studio_app;
use super::*;
use crate::chrome::settings::SettingsSection;
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
    assert!(
        lines[&remote("host-b")].text.contains("+"),
        "{:?}",
        lines[&remote("host-b")]
    );
    assert!(
        lines[&crate::chrome::snapshot::ClientEndpointId::Local]
            .text
            .contains("↻"),
        "{:?}",
        lines[&crate::chrome::snapshot::ClientEndpointId::Local]
    );

    publish_skill(
        &mut app,
        Some("host-b"),
        "0.4.32",
        rows(&[("codex", "current")]),
    );
    assert!(app.machine_lines()[&remote("host-b")].text.contains("✓"));
    app.view
        .documents_mut()
        .get_mut(&key(Some("host-b")))
        .and_then(|doc| std::sync::Arc::make_mut(doc).value.as_object_mut())
        .unwrap()
        .remove("agent_skill");
    assert!(app.machine_lines()[&remote("host-b")].text.contains("?"));
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
    assert!(text.contains("Agent skill"), "{text}");
    assert!(text.contains("host-b (codex, devin)"), "{text}");
    assert!(text.contains("↵ install"), "{text}");
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
    assert!(text.contains("enter replace…"), "{text}");
    assert!(!text.contains("Local —"), "{text}");
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
