//! Round-1 review fix integration tests for `backlot metrics` and `backlot digest`.

use std::fs;

use backlot_model::event::{EventKind, RefusalDisposition};
use unicode_width::UnicodeWidthStr;

use crate::cli_testkit::*;
use crate::metrics_cli_tests::{evt, now_ms, studio_ev, write_events};

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
