//! Hand-counted ledger metric tests over synthetic neutral studios.

use crate::attention::AttentionClass;
use crate::event::PERMISSION_PROMPT;
use crate::hollywood::{HollywoodEvent, HollywoodKind};
use crate::metrics::{METRICS_SCHEMA_VERSION, MetricsInput, StudioEventsInput, compute_metrics};
use crate::metrics_test_fx::acme_events;
use crate::testkit::{base_events, nhw};

fn hevt(seq: u64, ts_ms: u64, role: &str, kind: HollywoodKind) -> HollywoodEvent {
    nhw(role, kind).stamp(seq, ts_ms)
}

fn hmsg(
    seq: u64,
    ts_ms: u64,
    role: &str,
    studio: &str,
    file: Option<&str>,
    src: Option<u64>,
) -> HollywoodEvent {
    let kind = HollywoodKind::MessageSent {
        to: "hollywood".into(),
        studio: Some(studio.into()),
        text: file.is_none().then(|| "hi".into()),
        file: file.map(str::to_string),
        source_seq: src,
    };
    hevt(seq, ts_ms, role, kind)
}

fn hollywood_events() -> Vec<HollywoodEvent> {
    vec![
        hmsg(1, 50_000, "stage", "acme-shop", Some("h1.md"), Some(1)),
        hevt(
            2,
            50_500,
            "stage",
            HollywoodKind::MessageDropped {
                target: 1,
                reason: "stale".into(),
            },
        ),
        hmsg(3, 51_000, "stage", "beta-api", Some("h2.md"), Some(2)),
        hevt(
            4,
            51_500,
            "stage",
            HollywoodKind::MessageDelivered { target: 3 },
        ),
        hmsg(5, 52_000, "stage", "beta-api", Some("h3.md"), Some(2)),
        hevt(
            6,
            52_500,
            "stage",
            HollywoodKind::MessageDropped {
                target: 5,
                reason: "stale".into(),
            },
        ),
        hmsg(7, 53_000, "user", "acme-shop", None, None),
    ]
}

#[test]
fn hand_counted_metrics_across_studios() {
    let report = compute_metrics(&MetricsInput {
        studios: vec![
            StudioEventsInput {
                studio: "acme-shop".into(),
                events: acme_events(),
            },
            StudioEventsInput {
                studio: "beta-api".into(),
                events: base_events(),
            },
        ],
        hollywood_events: hollywood_events(),
        hollywood_include_unscoped: true,
        since_ms: None,
        now_ms: 200_000,
    });

    assert_eq!(report.schema_version, METRICS_SCHEMA_VERSION);
    assert_eq!(report.unknown_events_skipped, 1);
    let acme = &report.studios[0];
    assert_eq!(acme.unknown_events, 1);

    assert_eq!(acme.quality.delivered_tasks, 2);
    assert_eq!(acme.quality.first_delivery_accepted, 1);
    assert!((acme.quality.first_delivery_pass_rate.unwrap() - 0.5).abs() < 1e-9);
    assert_eq!(acme.quality.deliveries, 3);
    assert_eq!(acme.quality.changes_requested_reviews, 1);
    assert!((acme.quality.changes_requested_rate.unwrap() - (1.0 / 3.0)).abs() < 1e-9);
    assert_eq!(acme.quality.created_tasks, 2);
    assert_eq!(acme.quality.early_tasks, 1);

    assert!(acme.checks.configured);
    assert_eq!(acme.checks.configured_checks, 2);
    assert_eq!(acme.checks.task_checked, 2);
    assert_eq!(acme.checks.task_checked_passed, 1);
    assert_eq!(acme.checks.task_checked_failed, 1);
    assert_eq!(acme.findings.task_finding_events, 1);
    assert_eq!(acme.findings.findings, 2);

    assert_eq!(acme.continuity.director_actor_tells, 2);
    assert!((acme.continuity.tells_per_task.unwrap() - 1.0).abs() < 1e-9);

    let pp_is_stuck = AttentionClass::of(PERMISSION_PROMPT, false) == AttentionClass::Stuck;
    assert_eq!(acme.attention.total, 8);
    assert_eq!(acme.attention.open, 1);
    assert_eq!(
        acme.attention.by_class.needs_you.count,
        if pp_is_stuck { 1 } else { 2 }
    );
    assert_eq!(
        acme.attention.by_class.needs_you.open_ms,
        if pp_is_stuck { 2_000 } else { 3_500 }
    );
    assert_eq!(
        acme.attention.by_class.stuck.count,
        if pp_is_stuck { 6 } else { 5 }
    );
    assert_eq!(
        acme.attention.by_class.stuck.open_ms,
        if pp_is_stuck { 9_500 } else { 8_000 }
    );
    assert_eq!(acme.attention.by_class.info.count, 1);
    assert_eq!(acme.attention.by_class.info.open, 1);
    assert_eq!(acme.attention.by_class.info.open_ms, 130_000);
    assert_eq!(acme.attention.permission_prompts_not_needs_user.count, 1);
    assert_eq!(
        acme.attention.permission_prompts_not_needs_user.open_ms,
        1_500
    );
    assert_eq!(acme.attention.rate_limited.total, 4);
    assert_eq!(acme.attention.rate_limited.windows, 2);
    assert_eq!(acme.attention.rate_limited.max_per_window, 3);
    assert!((acme.attention.rate_limited.avg_per_window.unwrap() - 2.0).abs() < 1e-9);

    assert_eq!(acme.approvals.total, 3);
    assert_eq!(acme.approvals.allow, 2);
    assert_eq!(acme.approvals.deny, 1);
    assert_eq!(acme.refused.total, 4);
    assert_eq!(acme.refused.without_probes, 2);
    assert_eq!(acme.refused.probes_dropped_by_209, 2);
    assert_eq!(acme.refused.by_rule["probe-rule"].without_probes, 0);
    assert_eq!(acme.refused.by_rule["danger-rm"].without_probes, 1);

    assert_eq!(acme.hollywood.notices_sent, 1);
    assert_eq!(acme.hollywood.notices_dropped, 1);
    assert!((acme.hollywood.drop_rate.unwrap() - 1.0).abs() < 1e-9);

    assert_eq!(acme.timing.opening_ms, Some(3_000));
    assert_eq!(acme.timing.review_latency_samples, 3);
    assert_eq!(acme.timing.review_latency_p50_ms, Some(3_000));
    assert_eq!(acme.timing.review_latency_p90_ms, Some(8_600));
    assert_eq!(acme.timing.task_wall_clock_samples, 2);
    assert_eq!(acme.timing.task_wall_clock_p50_ms, Some(31_000));
    assert_eq!(acme.timing.pause_clusters, 1);
    assert_eq!(acme.timing.paused_tasks_in_clusters, 3);

    assert_eq!(report.all.studios_total, 2);
    assert_eq!(report.all.studios_with_checks, 1);
    assert!((report.all.checks_coverage_rate.unwrap() - 0.5).abs() < 1e-9);
    assert_eq!(report.all.hollywood.notices_sent, 3);
    assert_eq!(report.all.hollywood.notices_dropped, 2);
}

#[test]
fn since_window_excludes_older_events_and_caps_open_time() {
    let report = compute_metrics(&MetricsInput {
        studios: vec![StudioEventsInput {
            studio: "acme-shop".into(),
            events: acme_events(),
        }],
        hollywood_events: hollywood_events(),
        hollywood_include_unscoped: false,
        since_ms: Some(50_000),
        now_ms: 200_000,
    });
    let acme = &report.studios[0];
    assert_eq!(acme.quality.deliveries, 0);
    assert_eq!(acme.quality.created_tasks, 0);
    assert_eq!(acme.timing.opening_ms, None);
    assert_eq!(acme.timing.review_latency_samples, 1);
    assert_eq!(acme.attention.total, 8);
    assert_eq!(acme.hollywood.notices_sent, 1);
}

#[test]
fn metrics_json_shape_contains_expected_keys() {
    let report = compute_metrics(&MetricsInput {
        studios: vec![StudioEventsInput {
            studio: "acme-shop".into(),
            events: acme_events(),
        }],
        hollywood_events: hollywood_events(),
        hollywood_include_unscoped: false,
        since_ms: None,
        now_ms: 200_000,
    });
    let val = serde_json::to_value(&report).expect("serialize metrics report");
    assert_eq!(val["schema_version"], 1);
    assert!(val["studios"].is_array());
    assert!(val["all"].is_object());
    assert!(val["all"]["quality"]["first_delivery_pass_rate"].is_number());
    assert!(val["all"]["attention"]["by_class"]["needs_you"].is_object());
    assert!(val["all"]["attention"]["rate_limited"]["windows"].is_number());
    assert!(val["all"]["refused"]["without_probes"].is_number());
    assert!(val["all"]["hollywood"]["drop_rate"].is_number());
    assert!(val["all"]["timing"]["pause_clusters"].is_number());
}
