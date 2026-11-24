//! D5 panel tests for the working role's detail line (`PersonLine::Working`).

use super::*;
use crate::chrome::palette::Palette;
use crate::panel::config::StudioKeys;
use crate::panel::{Panel, PanelMode, render};
use crate::studio::StudioKey;
use backlot_model::summary::{RoleDoing, RoleProgress, RoleSummary, Summary, Work};
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
        progress: None,
        doing: None,
        activity: None,
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
    }
}

fn render_panel(summary: Summary, width: u16) -> Vec<String> {
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
fn fit_working_line_scales_across_three_widths_and_omits_missing_parts() {
    let now_ms = 60 * 60_000;
    let mut role = base_role(Work::Working, now_ms - 47 * 60_000);
    role.progress = Some(RoleProgress {
        text: "running the parity suite".into(),
        ts_ms: now_ms - 12 * 60_000,
    });
    role.doing = Some(RoleDoing {
        cmd: "cargo test".into(),
        since_ms: now_ms - 3 * 60_000,
    });

    // 1. Wide width: full line fits.
    let full = fit_working_line(&role, now_ms, 80);
    assert_eq!(
        full,
        "working 47m · \"running the parity suite\" 12m ago · cargo test 3m"
    );

    // 2. Medium width: progress inside quotes shortens with `…`, keeping doing.
    let mid = fit_working_line(&role, now_ms, 55);
    assert!(display_width(&mid) <= 55, "{mid}");
    assert!(mid.starts_with("working 47m · \""), "{mid}");
    assert!(mid.contains("…\" 12m ago · cargo test 3m"), "{mid}");

    // 3. Narrow width: drops `doing` first, then fits/shortens progress.
    let narrow = fit_working_line(&role, now_ms, 34);
    assert!(display_width(&narrow) <= 34, "{narrow}");
    assert!(!narrow.contains("cargo test"), "{narrow}");
    assert!(narrow.starts_with("working 47m · \""), "{narrow}");
    assert!(narrow.contains("12m ago"), "{narrow}");

    // 4. Tiny width: drops progress too and truncates head cleanly.
    let tiny = fit_working_line(&role, now_ms, 10);
    assert!(display_width(&tiny) <= 10, "{tiny}");
    assert!(!tiny.contains('·'), "{tiny}");

    // 5. Missing progress / doing / since_ms omit parts with no stray `·`.
    role.progress = None;
    assert_eq!(
        fit_working_line(&role, now_ms, 80),
        "working 47m · cargo test 3m"
    );
    role.doing = None;
    assert_eq!(fit_working_line(&role, now_ms, 80), "working 47m");
    role.since_ms = 0;
    role.progress = Some(RoleProgress {
        text: "wrapping up".into(),
        ts_ms: now_ms - 60_000,
    });
    assert_eq!(
        fit_working_line(&role, now_ms, 80),
        "working · \"wrapping up\" 1m ago"
    );
}

#[test]
fn panel_renders_working_detail_row_only_for_working_roles() {
    let now_ms = 60 * 60_000;
    let mut working = base_role(Work::Working, now_ms - 47 * 60_000);
    working.progress = Some(RoleProgress {
        text: "running the parity suite".into(),
        ts_ms: now_ms - 12 * 60_000,
    });
    working.doing = Some(RoleDoing {
        cmd: "cargo test".into(),
        since_ms: now_ms - 3 * 60_000,
    });

    // Wide panel shows full line at x + 4 under the person row.
    let wide = render_panel(studio_with(working.clone(), now_ms), 72);
    assert!(
        wide.iter()
            .any(|l| l
                .contains("working 47m · \"running the parity suite\" 12m ago · cargo test 3m")),
        "{wide:#?}"
    );

    // Medium panel shortens progress text inside quotes with `…`.
    let mid = render_panel(studio_with(working.clone(), now_ms), 58);
    assert!(
        mid.iter()
            .any(|l| l.contains("…\" 12m ago · cargo test 3m")),
        "{mid:#?}"
    );

    // Narrow panel drops `doing`.
    let narrow = render_panel(studio_with(working, now_ms), 38);
    let working_row = narrow
        .iter()
        .find(|l| l.contains("working 47m"))
        .expect("working row present");
    assert!(!working_row.contains("cargo test"), "{working_row}");

    // Non-working role keeps its existing single line (no `working ...` row).
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
