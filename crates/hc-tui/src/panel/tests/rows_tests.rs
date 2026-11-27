//! The U1 row model: bordered blocks, dividers between groups only,
//! stacking when the blocks stop fitting, manual overrides and the
//! collapsed strip.
use super::*;
use crate::panel::data::StudioRef;
use crate::panel::row_lines::person_drawer;
use crate::panel::rows::{Owed, Target, rows};
use crate::panel::rows_fit::mark_counts;
use crate::panel::{Panel, PanelMode};
use crate::studio::StudioKey;
use crate::studio::glyphs::Person;

/// The studio ref the drawer-line tests read the fixture through.
fn studio_ref(summary: &Summary) -> StudioRef<'_> {
    StudioRef {
        machine: None,
        studio: &summary.studios[0],
    }
}

/// The stage's own answer rides the role's drawer as one dim line —
/// `trust answered by stage · 14:02` — and only when one happened.
/// It is never part of the attention line above it.
#[test]
fn a_stage_answered_role_draws_one_extra_dim_drawer_line() {
    let summary = summary();
    let s = studio_ref(&summary);
    let producer = &s.studio.roles[0];
    let lines = person_drawer(s, producer);
    assert_eq!(lines.len(), 3, "{lines:?}");

    let mut value = summary_json();
    value["studios"][0]["roles"][0]["stage_answer_ms"] = serde_json::json!(43_200_000);
    let summary: Summary = serde_json::from_value(value).unwrap();
    let s = studio_ref(&summary);
    let lines = person_drawer(s, &s.studio.roles[0]);
    assert_eq!(lines.len(), 4, "{lines:?}");
    let answer = &lines[2];
    assert!(answer.starts_with("trust answered by stage · "), "{answer}");
    let clock = answer.trim_start_matches("trust answered by stage · ");
    assert_eq!(clock.len(), 5, "HH:MM only: {clock}");
    assert!(clock[..2].chars().all(|c| c.is_ascii_digit()), "{clock}");
    assert_eq!(&clock[2..3], ":", "{clock}");
    assert!(clock[3..].chars().all(|c| c.is_ascii_digit()), "{clock}");
    // The attention line is untouched — the answer is not a cue.
    assert!(!lines[1].starts_with('!'), "{:?}", lines);
}

fn panel(mode: PanelMode) -> Panel {
    Panel {
        mode,
        ..Panel::new()
    }
}

fn targets(
    summary: Option<&Summary>,
    stage: bool,
    panel: &Panel,
    current: Option<&str>,
    body_height: u16,
) -> Vec<Target> {
    let current = current.map(StudioKey::local);
    rows(
        &machines(summary),
        |_| stage,
        |_| true,
        panel,
        panel.mode,
        current.as_ref(),
        body_height,
    )
    .iter()
    .map(|r| r.target())
    .collect()
}

fn people(targets: &[Target]) -> Vec<String> {
    targets
        .iter()
        .filter_map(|t| match t {
            Target::Person(_, role) => Some(role.clone()),
            _ => None,
        })
        .collect()
}

#[test]
fn expanded_block_lists_people_with_frame_rows_and_group_dividers() {
    let summary = summary();
    let t = targets(
        Some(&summary),
        true,
        &panel(PanelMode::Expanded),
        Some("acme-shop"),
        100,
    );
    assert_eq!(t.first(), Some(&Target::None));
    // The top border is the only `Studio` row — the title, the three
    // role-group dividers, the input-divider and the bottom border are the
    // `Target::None` rows (A2.2). Every live/standby/calm role renders on its
    // own selectable row regardless of sidebar state.
    assert_eq!(
        t.iter()
            .filter(|x| **x == Target::Studio("acme-shop".into()))
            .count(),
        1,
        "{t:?}"
    );
    assert_eq!(
        t.iter().filter(|x| **x == Target::None).count(),
        6,
        "title + three dividers + input-divider + bottom: {t:?}"
    );
    assert_eq!(
        people(&t),
        vec![
            "producer",
            "director",
            "writer",
            "actor-alice",
            "actor-erin",
            "actor-bob",
            "actor-carol",
        ]
    );
    assert!(
        t.contains(&Target::ClosedFold("acme-shop".into())),
        "closed seats fold behind ClosedFold by default: {t:?}"
    );
    let mut open_closed = panel(PanelMode::Expanded);
    open_closed.closed_folds.insert("acme-shop".into());
    let t_open = targets(Some(&summary), true, &open_closed, Some("acme-shop"), 100);
    assert!(people(&t_open).iter().any(|r| r == "actor-dave"));
    // Every person row stays strictly one line even with an attention.
    let mut value = summary_json();
    value["studios"][0]["roles"][3]["attention"] = serde_json::json!("needs permission");
    let summary: Summary = serde_json::from_value(value).unwrap();
    let t2 = targets(
        Some(&summary),
        true,
        &panel(PanelMode::Expanded),
        Some("acme-shop"),
        100,
    );
    assert_eq!(
        t2.iter()
            .filter(|x| **x == Target::Person("acme-shop".into(), "actor-alice".into()))
            .count(),
        1,
        "attention stays inline on the single Person row: {t2:?}"
    );
}

#[test]
fn stacking_kicks_in_when_blocks_do_not_fit_and_keeps_current_open() {
    let summary = summary();
    // Body too short for the expanded block → only the current studio
    // stays expanded, everything else is a `╶` line.
    let t = targets(
        Some(&summary),
        true,
        &panel(PanelMode::Expanded),
        Some("acme-shop"),
        5,
    );
    assert!(
        t.iter().any(|x| matches!(x, Target::Person(..))),
        "current stays expanded: {t:?}"
    );
    // No current → the block stacks to a single Block row.
    let t = targets(Some(&summary), true, &panel(PanelMode::Expanded), None, 5);
    assert_eq!(
        t,
        vec![Target::None, Target::Block("acme-shop".into())],
        "{t:?}"
    );
}

#[test]
fn space_overrides_the_automatic_block_state() {
    let summary = summary();
    // Manual collapse wins over the everything-fits expansion.
    let mut p = panel(PanelMode::Expanded);
    p.collapsed_blocks.insert("acme-shop".into());
    let t = targets(Some(&summary), true, &p, Some("acme-shop"), 100);
    assert_eq!(t, vec![Target::None, Target::Block("acme-shop".into())]);
    // Manual expand wins over the stacking collapse.
    let mut p = panel(PanelMode::Expanded);
    p.expanded_blocks.insert("acme-shop".into());
    let t = targets(Some(&summary), true, &p, None, 5);
    assert!(
        t.iter().any(|x| matches!(x, Target::Person(..))),
        "manual expand reopens the block: {t:?}"
    );
}

#[test]
fn collapsed_strip_lists_the_studio_and_its_busy_roles() {
    let summary = summary();
    let t = targets(Some(&summary), true, &panel(PanelMode::Collapsed), None, 0);
    // Every strip row — the studio line and its active/blocked role
    // lines — shares the studio's Strip target (A4.3). The fixture's
    // busy roles: producer (working), alice (attention), bob
    // (dir-prompt) and erin (blocked upstream? — whoever works).
    assert_eq!(t[0], Target::None);
    assert!(
        t[1..]
            .iter()
            .all(|x| *x == Target::Strip("acme-shop".into())),
        "{t:?}"
    );
    assert!(t.len() > 2, "the strip is never blank: {t:?}");
}

#[test]
fn mark_counts_follow_the_mark_order() {
    let summary = summary();
    let counts = mark_counts(&summary.studios[0]);
    assert_eq!(
        counts,
        vec![
            (Person::DirPrompt, 1),
            (Person::Lost, 1),
            (Person::Working, 3),
            (Person::Ready, 2),
            (Person::Resumable, 1),
        ]
    );
}

#[test]
fn missing_document_is_title_plus_no_studios() {
    assert_eq!(
        targets(None, true, &panel(PanelMode::Expanded), None, 0),
        vec![Target::None, Target::None]
    );
}

#[test]
fn a_dead_stage_moves_the_stage_word_inside_the_block() {
    let summary = summary();
    let alive = targets(
        Some(&summary),
        true,
        &panel(PanelMode::Expanded),
        Some("acme-shop"),
        100,
    );
    let dead = targets(
        Some(&summary),
        false,
        &panel(PanelMode::Expanded),
        Some("acme-shop"),
        100,
    );
    // No top `stage stopped` row — the owed subheader inside the
    // block carries it now (S2 §2): one extra row under the block's
    // top border — the clickable `Stage` target U6c gave it — every
    // other row untouched.
    assert_eq!(dead.len(), alive.len() + 1, "{dead:?}");
    assert_eq!(dead[0], Target::None);
    assert_eq!(dead[1], alive[1]);
    assert_eq!(
        dead[2],
        Target::Stage(None),
        "the owed line joins: {dead:?}"
    );
    assert_eq!(&dead[3..], &alive[2..]);
}

#[test]
fn idle_collapsed_strip_emits_one_row_when_healthy_and_summary_when_stage_down() {
    let value = serde_json::json!({
        "schema": "backlot/1",
        "updated_ms": 1,
        "studios": [{
            "id": "acme-shop",
            "title": "Acme shop",
            "state": "live",
            "requirement_version": 1,
            "mode": "standard",
            "open_reviews": 0,
            "waiting": 0,
            "roles": [
                { "role": "producer", "name": "producer", "terminal": "t1", "state": "live", "work": "waiting" },
                { "role": "director", "name": "director", "terminal": "t2", "state": "live", "work": "waiting" },
                { "role": "actor-a", "name": "alice", "state": "closed", "resumable": true }
            ]
        }]
    });
    let summary: Summary = serde_json::from_value(value).unwrap();
    let list = machines(Some(&summary));
    let p = panel(PanelMode::Collapsed);
    let r = rows(&list, |_| true, |_| true, &p, PanelMode::Collapsed, None, 0);
    assert_eq!(r.len(), 2, "calm studio takes 1 strip line only: {r:?}");
    assert!(matches!(r[1], crate::panel::Row::Strip { .. }));

    // Stage down keeps 1 summary line beneath the studio strip row.
    let down = rows(
        &list,
        |_| false,
        |_| true,
        &p,
        PanelMode::Collapsed,
        None,
        0,
    );
    assert!(
        down.iter()
            .any(|row| matches!(row, crate::panel::Row::StripSummary { .. })),
        "stage-down strip emits StripSummary: {down:?}"
    );
    assert!(
        down.iter()
            .skip(1)
            .all(|row| row.target() == Target::Strip("acme-shop".into())),
        "StripSummary shares Strip target"
    );
}

/// R3-3/R3-M10: a `ready_to_finish` studio owes `finish studio ·
/// user [f]` ahead of the post-acceptance `node report` step — the
/// ledger step must not hide the `f` — and a dim `report pending`
/// line rides under the offer while the wrap report is still owed.
#[test]
fn a_ready_studio_owes_finish_while_its_wrap_report_is_pending() {
    use crate::panel::rows::{Owed, Row};
    let mut pending = summary();
    pending.studios[0].ready_to_finish = true;
    pending.studios[0].next = Some("node report — director".into());
    let p = panel(PanelMode::Expanded);
    let pending_machines = machines(Some(&pending));
    let out = rows(&pending_machines, |_| true, |_| true, &p, p.mode, None, 40);
    let kinds: Vec<Owed> = out
        .iter()
        .filter_map(|row| match row {
            Row::Owed { kind, .. } => Some(*kind),
            _ => None,
        })
        .collect();
    assert_eq!(kinds, [Owed::Finish, Owed::ReportPending], "{out:?}");

    // The wrap report landed — `next` is a normal ledger step again:
    // the offer stays, the pending cue goes.
    let mut done = summary();
    done.studios[0].ready_to_finish = true;
    let done_machines = machines(Some(&done));
    let out = rows(&done_machines, |_| true, |_| true, &p, p.mode, None, 40);
    let kinds: Vec<Owed> = out
        .iter()
        .filter_map(|row| match row {
            Row::Owed { kind, .. } => Some(*kind),
            _ => None,
        })
        .collect();
    assert_eq!(kinds, [Owed::Finish], "{out:?}");
}

/// R3-3: `f` armed on a ready studio asks `finish <id>? [y/n]` —
/// while the wrap report is still owed the ask itself warns.
#[test]
fn the_armed_finish_confirm_warns_while_the_report_pends() {
    use crate::panel::rows::{Owed, Row};
    let mut armed = panel(PanelMode::Expanded);
    armed.finish_confirm = Some(StudioKey::local("acme-shop"));
    let confirm_text = |summary: &Summary| {
        let machines = machines(Some(summary));
        let out = rows(&machines, |_| true, |_| true, &armed, armed.mode, None, 40);
        out.iter()
            .filter_map(|row| match row {
                Row::Owed {
                    kind: Owed::Confirm,
                    text,
                    ..
                } => Some(text.clone()),
                _ => None,
            })
            .next()
            .unwrap_or_default()
    };
    let mut pending = summary();
    pending.studios[0].ready_to_finish = true;
    pending.studios[0].next = Some("node report — director".into());
    assert_eq!(
        confirm_text(&pending),
        "finish acme-shop (report pending)? [y/n]"
    );
    let mut landed = summary();
    landed.studios[0].ready_to_finish = true;
    assert_eq!(confirm_text(&landed), "finish acme-shop? [y/n]");
}

/// R3-M5: a brand-new install — no machine's document parsed — still
/// shows the expanded dock's `⏎ hollywood setup` row; the collapsed
/// strip does not.
#[test]
fn a_first_run_dock_names_the_setup_row() {
    let mut p = panel(PanelMode::Expanded);
    p.hollywood_hint = true;
    let empty = machines(None);
    let out = rows(&empty, |_| true, |_| true, &p, p.mode, None, 24);
    assert!(
        out.iter()
            .any(|row| matches!(row, crate::panel::Row::SetupHint)),
        "{out:?}"
    );
    assert!(
        out.iter().any(|row| row.target() == Target::Setup),
        "the row is actionable: {out:?}"
    );
    let mut strip = panel(PanelMode::Collapsed);
    strip.hollywood_hint = true;
    let out = rows(&empty, |_| true, |_| true, &strip, strip.mode, None, 24);
    assert!(
        out.iter()
            .all(|row| !matches!(row, crate::panel::Row::SetupHint)),
        "{out:?}"
    );
}

/// The lot studio row carries the studio's own next step: a studio
/// with no requirement says to tell the producer what to build.
#[test]
fn the_studio_row_shows_its_next_step() {
    use crate::panel::rows::Row;
    let mut value = summary_json();
    value["studios"][0]["requirement_version"] = serde_json::json!(0);
    let summary: Summary = serde_json::from_value(value).unwrap();
    let p = panel(PanelMode::Expanded);
    let all = machines(Some(&summary));
    let out = rows(&all, |_| true, |_| true, &p, p.mode, None, 40);
    assert!(
        out.iter().any(|row| matches!(
            row,
            Row::Owed {
                kind: Owed::Next,
                text,
                ..
            } if text == "next: tell the producer what to build"
        )),
        "{out:?}"
    );
}

#[test]
fn a_studio_needing_upgrade_shows_a_single_owed_line_in_lot() {
    use crate::panel::rows::Row;
    let mut value = summary_json();
    value["studios"][0]["needs_upgrade"] = serde_json::json!(true);
    value["studios"][0]["unknown_kinds"] = serde_json::json!(["design.opened"]);
    value["studios"][0]["ready_to_finish"] = serde_json::json!(false);
    let summary: Summary = serde_json::from_value(value).unwrap();
    let p = panel(PanelMode::Expanded);
    let all = machines(Some(&summary));
    let out = rows(&all, |_| true, |_| true, &p, p.mode, None, 40);
    let upgrade_rows: Vec<&String> = out
        .iter()
        .filter_map(|row| match row {
            Row::Owed { text, .. } if text.contains("upgrade backlot") => Some(text),
            _ => None,
        })
        .collect();
    assert_eq!(upgrade_rows.len(), 1, "{out:?}");
}

#[test]
fn spacer_separates_blocks_and_compact_groups_and_drops_first_under_height_pressure() {
    use crate::panel::rows::Row;
    let calm_studio = |id: &str| {
        serde_json::json!({
            "id": id,
            "title": id,
            "state": "live",
            "requirement_version": 1,
            "mode": "standard",
            "open_reviews": 0,
            "waiting": 0,
            "tasks_done": 1,
            "tasks_total": 1,
            "roles": [
                { "role": "producer", "name": "producer", "terminal": "t1", "state": "live", "work": "waiting" }
            ],
            "tasks": [
                { "id": "t1", "title": "First", "status": "accepted", "requirement_version": 1, "priority": "normal", "rework_rounds": 0, "rework_cap": 0 }
            ]
        })
    };
    let mut base = summary_json();
    base["studios"][0]["exchange"] = serde_json::json!([
        { "seq": 1, "who": "user", "text": "ship it" }
    ]);
    let s1 = base["studios"][0].clone();
    let mut s2 = base["studios"][0].clone();
    s2["id"] = serde_json::json!("blog");
    s2["exchange"] = serde_json::json!([]);
    let doc: Summary = serde_json::from_value(serde_json::json!({
        "schema": "backlot/1",
        "updated_ms": 1,
        "studios": [s1, s2, calm_studio("gamma-web"), calm_studio("delta-ops")],
    }))
    .unwrap();
    let list = machines(Some(&doc));
    let p = panel(PanelMode::Expanded);

    // Abundant height: spacer between Block 1 and Block 2, and between Block 2 and Compact group;
    // NO spacer between adjacent Compact rows (`gamma-web`, `delta-ops`).
    let roomy = rows(
        &list,
        |_| true,
        |_| true,
        &p,
        p.mode,
        Some(&StudioKey::local("acme-shop")),
        60,
    );
    let spacer_positions: Vec<usize> = roomy
        .iter()
        .enumerate()
        .filter_map(|(i, r)| matches!(r, Row::Spacer).then_some(i))
        .collect();
    assert_eq!(spacer_positions.len(), 2, "expected 2 spacers: {roomy:?}");
    assert!(matches!(
        roomy[spacer_positions[0] - 1],
        Row::BlockBottom(_)
    ));
    assert!(matches!(
        roomy[spacer_positions[0] + 1],
        Row::BlockTop { .. }
    ));
    assert!(matches!(
        roomy[spacer_positions[1] - 1],
        Row::BlockBottom(_)
    ));
    assert!(matches!(
        roomy[spacer_positions[1] + 1],
        Row::CompactStudio { .. }
    ));
    assert!(matches!(
        roomy[spacer_positions[1] + 2],
        Row::CompactStudio { .. }
    ));

    // Exact tight height (`body_rows` excluding spacers): spacers drop BEFORE talk turns or folding!
    let content_count = roomy
        .iter()
        .skip(1)
        .filter(|r| !matches!(r, Row::Spacer))
        .count();
    let tight = rows(
        &list,
        |_| true,
        |_| true,
        &p,
        p.mode,
        Some(&StudioKey::local("acme-shop")),
        content_count as u16,
    );
    assert!(
        !tight.iter().any(|r| matches!(r, Row::Spacer)),
        "spacers must drop under height pressure"
    );
    assert!(
        tight.iter().any(|r| matches!(r, Row::Talk { .. })),
        "talk content survives ahead of blank spacer rows"
    );
    assert!(
        tight.iter().any(|r| matches!(r, Row::CompactStudio { .. })),
        "compact content survives ahead of blank spacer rows"
    );
}
