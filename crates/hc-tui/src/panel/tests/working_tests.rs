//! Tests for inline working progress/running detail and wall-clock idle/working ages.

use super::*;
use crate::chrome::palette::Palette;
use crate::panel::config::StudioKeys;
use crate::panel::{Panel, PanelMode, render};
use crate::studio::StudioKey;
use backlot_model::summary::{RoleProgress, RoleRunning, RoleSummary, Summary, Work};
use ratatui::buffer::Buffer;
use ratatui::layout::Rect;

fn base_role(work: Work, since_ms: u64) -> RoleSummary {
    RoleSummary {
        role: "actor-t1-c1".into(),
        name: "actor-a".into(),
        terminal: Some("t2".into()),
        runner: Some("claude".into()),
        model: Some("sonnet".into()),
        effort: None,
        state: "live".into(),
        doing: None,
        progress: None,
        running: None,
        work: Some(work),
        failed: None,
        workdir: None,
        task: Some("t1".into()),
        priority: None,
        resumable: false,
        since_ms,
        attention: None,
        attention_class: None,
        asks: None,
        attention_user: false,
        attention_seq: None,
        attention_detail: None,
        stage_answer_ms: None,
        can_answer_prompt: true,
        restarted_ms: None,
        idle_since_ms: None,
        account: None,
    }
}

fn render_panel(summary: Summary, width: u16) -> Vec<String> {
    let now_ms = summary.updated_ms;
    render_panel_at(summary, width, now_ms)
}

fn render_panel_at(summary: Summary, width: u16, now_ms: u64) -> Vec<String> {
    let area = Rect::new(0, 0, width, 16);
    let mut buf = Buffer::empty(area);
    let panel = Panel {
        mode: PanelMode::Expanded,
        ..Panel::new()
    };
    let current = StudioKey::local("acme");
    let machine = crate::panel::data::MachineSummary {
        machine: None,
        summary,
    };
    render(
        &mut buf,
        area,
        &[machine],
        |_| true,
        |_| true,
        &panel,
        PanelMode::Expanded,
        Some(&current),
        Some(&current),
        1,
        None,
        &StudioKeys::default(),
        None,
        false,
        false,
        true,
        &Default::default(),
        &Palette::from_name("catppuccin").unwrap(),
        now_ms,
    );
    (area.y..area.y + area.height)
        .map(|y| {
            (area.x..area.x + area.width)
                .map(|x| buf[(x, y)].symbol())
                .collect::<String>()
                .trim_end()
                .to_string()
        })
        .collect()
}

fn studio_with(role: RoleSummary, updated_ms: u64) -> Summary {
    serde_json::from_value(serde_json::json!({
        "schema": "backlot/1",
        "updated_ms": updated_ms,
        "studios": [{
            "id": "acme",
            "title": "acme",
            "state": "live",
            "requirement_version": 1,
            "mode": "standard",
            "open_reviews": 0,
            "waiting": 0,
            "ready_to_finish": false,
            "tasks_done": 0,
            "tasks_total": 1,
            "tasks": [],
            "roles": [role]
        }]
    }))
    .unwrap()
}

#[test]
fn panel_renders_inline_working_detail_with_progress_priority_and_omits_extra_row() {
    let now_ms = 60 * 60_000;
    let mut working = base_role(Work::Working, now_ms - 47 * 60_000);
    working.progress = Some(RoleProgress {
        text: "running the parity suite".into(),
        ts_ms: now_ms - 12 * 60_000,
    });
    working.running = Some(RoleRunning {
        cmd: "cargo test".into(),
        since_ms: now_ms - 3 * 60_000,
    });
    working.doing = Some("writing code".into());

    // Wide panel renders `progress` in the 1-line inline tail, not a 2nd row.
    let wide = render_panel(studio_with(working.clone(), now_ms), 64);
    let working_rows: Vec<&String> = wide
        .iter()
        .filter(|l| l.contains("working 47m"))
        .collect();
    assert_eq!(working_rows.len(), 1, "{wide:#?}");
    assert!(
        working_rows[0].contains("working 47m · running the parity suite"),
        "{}",
        working_rows[0]
    );

    // Narrow panel truncates detail with `…`, never pushing `working` off.
    let narrow = render_panel(studio_with(working.clone(), now_ms), 34);
    let row = narrow.iter().find(|l| l.contains("working 47m")).unwrap();
    assert!(row.contains("…"), "{row}");

    // Without progress, falls back to `running.cmd`.
    working.progress = None;
    let fallback = render_panel(studio_with(working, now_ms), 54);
    assert!(
        fallback
            .iter()
            .any(|l| l.contains("working 47m · cargo test")),
        "{fallback:#?}"
    );

    // Non-working role keeps idle state without stale working progress.
    let mut idle = base_role(Work::Waiting, now_ms - 15 * 60_000);
    idle.progress = Some(RoleProgress {
        text: "leftover note".into(),
        ts_ms: now_ms - 12 * 60_000,
    });
    let idle_lines = render_panel(studio_with(idle, now_ms), 72);
    assert!(
        !idle_lines.iter().any(|l| l.contains("leftover note")),
        "{idle_lines:#?}"
    );
}

/// A live role that has sat at its prompt past `IDLE_SHOWN_MS`
/// shows `idle <n>m` in its dock row; under the floor it shows `idle`.
#[test]
fn a_role_at_its_prompt_reads_idle_minutes_in_the_dock() {
    let now_ms = 90 * 60_000;
    let mut role = base_role(Work::Waiting, now_ms - 30 * 60_000);
    role.idle_since_ms = Some(now_ms - 4 * 60_000);
    let lines = render_panel(studio_with(role, now_ms), 72);
    let row = lines
        .iter()
        .find(|l| l.contains("Ⓐ t1"))
        .expect("person row present");
    assert!(row.contains("idle 4m"), "{row}");

    // Under the floor it stays plain `idle`, not `idle 0s`.
    let mut fresh = base_role(Work::Waiting, now_ms - 30 * 60_000);
    fresh.idle_since_ms = Some(now_ms - 30_000);
    let lines = render_panel(studio_with(fresh, now_ms), 72);
    let row = lines.iter().find(|l| l.contains("Ⓐ t1")).unwrap();
    assert!(row.contains("idle"), "{row}");
    assert!(!row.contains("0s"), "{row}");
}

#[test]
fn working_ages_advance_with_injected_wall_clock_for_fixed_summary() {
    let updated_ms = 60 * 60_000;
    let mut role = base_role(Work::Working, updated_ms - 10 * 60_000);
    role.progress = Some(RoleProgress {
        text: "running the parity suite".into(),
        ts_ms: updated_ms - 2 * 60_000,
    });
    let fixed_summary = studio_with(role, updated_ms);

    let at_t1 = render_panel_at(fixed_summary.clone(), 80, updated_ms);
    assert!(
        at_t1
            .iter()
            .any(|l| l.contains("working 10m · running the parity suite")),
        "{at_t1:#?}"
    );

    // Same fixed summary (same `updated_ms`), 5 minutes later by injected wall clock.
    let at_t2 = render_panel_at(fixed_summary.clone(), 80, updated_ms + 5 * 60_000);
    assert!(
        at_t2
            .iter()
            .any(|l| l.contains("working 15m · running the parity suite")),
        "{at_t2:#?}"
    );

    // Remote clock slightly ahead of local wall clock never yields negative ages (`0s` clamp).
    let at_early = render_panel_at(fixed_summary, 80, updated_ms - 30 * 60_000);
    assert!(
        at_early
            .iter()
            .any(|l| l.contains("working 0s · running the parity suite")),
        "{at_early:#?}"
    );
}
