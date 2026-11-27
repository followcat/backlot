//! Round-1 review fix integration tests for `backlot metrics` and `backlot digest`.

use std::fs;

use backlot_model::event::{EventKind, RefusalDisposition, TaskStateChange, Verdict};
use unicode_width::UnicodeWidthStr;

use crate::cli_testkit::*;
use crate::metrics_cli_tests::{deliv, evt, now_ms, rev, studio_ev, write_events};

#[test]
fn digest_handles_cjk_acceptance_titles_and_formats_reset_human_readable() {
    let (scratch, env) = scratch();
    let now = now_ms();
    let recent = now.saturating_sub(3_600_000);
    let reset_ms = now + 15 * 60_000;
    let sdir = crate::store::studio_dir(&scratch.dir, "acme-shop");

    fs::create_dir_all(sdir.join("tasks/t1")).unwrap();
    fs::write(
        sdir.join("tasks/t1/acceptance.md"),
        "验收：已完成 design v1 §R1 的接口，见 design v2 §R2.W",
    )
    .unwrap();

    let events = vec![
        studio_ev(1, recent, "acme-shop", "/home/user/acme-shop", vec![]),
        evt(
            2,
            recent + 1_000,
            "director",
            EventKind::TaskCreated {
                task: "t1".into(),
                title: "实现中文标题与接口验收".into(),
                task_kind: "code".into(),
                requirement_version: 1,
                acceptance: "tasks/t1/acceptance.md".into(),
                dependencies: vec![],
            },
        ),
        evt(
            3,
            recent + 2_000,
            "stage",
            EventKind::AttentionRaised {
                role: "director".into(),
                reason: "rate limited".into(),
                detail: "quota hit".into(),
                needs_user: false,
                reset_at_ms: Some(reset_ms),
                account: None,
            },
        ),
        evt(
            4,
            recent + 3_000,
            "stage",
            EventKind::RoleRefused {
                role: "actor-t1-c1".into(),
                task: Some("t1".into()),
                command: "echo 中文".into(),
                cwd: "/home/user/acme-shop".into(),
                argv: vec!["echo".into(), "中文".into()],
                rule: Some("中文规则限制名称".repeat(8)),
                disposition: RefusalDisposition::Refused,
            },
        ),
    ];
    write_events(&sdir, &events, None);

    let table = ok(&env, "metrics --studio acme-shop");
    for line in table.lines() {
        assert!(
            UnicodeWidthStr::width(line) <= 100,
            "line exceeded 100 display cols ({}): {line}",
            UnicodeWidthStr::width(line)
        );
    }

    let md = ok(&env, "digest --studio acme-shop");
    assert!(md.contains("design v1 §R1"), "{md}");
    assert!(md.contains("design v2 §R2.W"), "{md}");
    assert!(md.contains("checks: none"), "{md}");
    let expected_hhmm = crate::ratelimit::format_reset_local(reset_ms, now);
    assert!(
        md.contains(&format!("until {expected_hhmm}")),
        "expected readable 'until {expected_hhmm}', got:\n{md}"
    );
    assert!(
        !md.contains("reset_at_ms:"),
        "digest must not print raw reset_at_ms:\n{md}"
    );
}

#[test]
fn digest_caps_long_cjk_review_note_to_120_cols_and_keeps_full_json() {
    let (scratch, env) = scratch();
    let now = now_ms();
    let recent = now.saturating_sub(3_600_000);
    let sdir = crate::store::studio_dir(&scratch.dir, "acme-shop");

    let long_cjk_note: String = "请修复边界处理异常".repeat(334); // 3006 CJK chars
    assert!(long_cjk_note.chars().count() >= 3000);
    let long_title = "超长中文任务标题".repeat(20);
    let long_reason = "第一句阻塞说明。\n第二句不应该出现在摘要行里，且继续很长".repeat(5);

    let events = vec![
        studio_ev(1, recent, "acme-shop", "/home/user/acme-shop", vec![]),
        evt(
            2,
            recent + 1_000,
            "director",
            EventKind::TaskCreated {
                task: "t1".into(),
                title: long_title.clone(),
                task_kind: "code".into(),
                requirement_version: 1,
                acceptance: "tasks/t1/acceptance.md".into(),
                dependencies: vec![],
            },
        ),
        deliv(3, recent + 2_000, "t1", 1, "abc123..def456"),
        rev(
            4,
            recent + 3_000,
            "t1",
            1,
            Verdict::ChangesRequested,
            &long_cjk_note,
        ),
        evt(
            5,
            recent + 4_000,
            "director",
            EventKind::TaskCreated {
                task: "t2".into(),
                title: "Second task. Extra detail should not leak.".into(),
                task_kind: "code".into(),
                requirement_version: 1,
                acceptance: "tasks/t2/acceptance.md".into(),
                dependencies: vec![],
            },
        ),
        evt(
            6,
            recent + 5_000,
            "director",
            EventKind::TaskState {
                task: "t2".into(),
                state: TaskStateChange::Blocked,
                reason: long_reason.clone(),
            },
        ),
    ];
    write_events(&sdir, &events, None);

    let md = ok(&env, "digest --studio acme-shop");
    let sent_back_line = md
        .lines()
        .find(|l| l.starts_with("- `t1` d1 (by director): "))
        .expect("sent back bullet line");
    assert!(
        UnicodeWidthStr::width(sent_back_line) <= 120,
        "sent back line too wide ({}): {sent_back_line}",
        UnicodeWidthStr::width(sent_back_line)
    );
    assert!(
        sent_back_line.ends_with('…'),
        "expected trailing ellipsis: {sent_back_line}"
    );
    // All sent_back lines (just the single bullet) must stay <= 120 display cols.
    let deliv_line = md
        .lines()
        .find(|l| l.starts_with("- `t1` d1: `abc123..def456`"))
        .expect("delivered bullet line");
    assert!(
        UnicodeWidthStr::width(deliv_line) <= 120 && deliv_line.ends_with('…'),
        "delivered line width={} line={deliv_line}",
        UnicodeWidthStr::width(deliv_line)
    );
    let blocked_line = md
        .lines()
        .find(|l| l.starts_with("- `t2`: "))
        .expect("blocked bullet line");
    assert!(
        UnicodeWidthStr::width(blocked_line) <= 120 && !blocked_line.contains("第二句"),
        "blocked line width={} line={blocked_line}",
        UnicodeWidthStr::width(blocked_line)
    );

    let json_raw = ok(&env, "digest --studio acme-shop --format json");
    let v: serde_json::Value = serde_json::from_str(&json_raw).unwrap();
    assert_eq!(
        v["studios"][0]["sent_back"][0]["note"].as_str().unwrap(),
        long_cjk_note
    );
    assert_eq!(
        v["studios"][0]["blocked"][0]["reason"].as_str().unwrap(),
        long_reason
    );
}
