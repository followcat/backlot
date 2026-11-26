//! Studio Dock exchange rows: `›` (user) vs `✦` (producer /
//! hollywood) glyphs, selection highlight inside `│ … │`, contextual
//! `⏎ read` footer hint when `file.is_some()`, and `rows_fit` height
//! degradation so narrow docks never starve roles, tasks or `›`.

use super::*;
use crate::chrome::palette::Palette;
use crate::panel::config::StudioKeys;
use crate::panel::rows::{Target, rows};
use crate::panel::{Panel, PanelMode, render};
use ratatui::buffer::Buffer;
use ratatui::layout::Rect;

fn summary_with_exchange() -> Summary {
    let mut v = summary_json();
    v["studios"][0]["exchange"] = serde_json::json!([
        { "seq": 1, "who": "user", "text": "check cart latency", "file": "messages/1-tell.txt" },
        { "seq": 2, "who": "hollywood", "text": "prioritize p99 regressions", "file": "messages/2-hw.md" },
        { "seq": 3, "who": "producer", "text": "cast alice on task-1", "file": "messages/3-reply.md" },
        { "seq": 4, "who": "user", "text": "any blockers?" },
        { "seq": 5, "who": "producer", "text": "milestone-1 green", "file": "reports/5-m1.md" },
        { "seq": 6, "who": "producer", "text": "ready for final check", "file": "messages/6-ready.md" }
    ]);
    serde_json::from_value(v).unwrap()
}

fn lines(buf: &Buffer, area: Rect) -> Vec<String> {
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

#[test]
fn exchange_rows_render_glyphs_selection_and_contextual_read_hint() {
    let summary = summary_with_exchange();
    let palette = Palette::from_name("catppuccin").unwrap();
    let area = Rect::new(0, 0, 38, 28);
    let mut panel = Panel {
        mode: PanelMode::Expanded,
        focused: true,
        selected: Some(Target::Talk("acme-shop".into(), 3)),
        ..Panel::new()
    };
    let mut buf = Buffer::empty(area);
    let current = crate::studio::StudioKey::local("acme-shop");
    let hits = render(
        &mut buf,
        area,
        &machines(Some(&summary)),
        |_| true,
        |_| true,
        &panel,
        PanelMode::Expanded,
        Some(&current),
        Some(&current),
        0,
        None,
        &StudioKeys::default(),
        None,
        false,
        false,
        true,
        &Default::default(),
        &palette,
        1,
    );
    let rendered = lines(&buf, area);
    let text = rendered.join("\n");
    assert!(
        rendered
            .iter()
            .any(|l| l.contains("› check cart latency") && l.ends_with('│')),
        "{text}"
    );
    assert!(
        rendered
            .iter()
            .any(|l| l.contains("✦ prioritize p99") && l.ends_with('│')),
        "{text}"
    );
    // Selected Talk(#3) paints selection_bg inside `│ … │` with `▌` inside the block border.
    let (sel_rect, _) = hits
        .rows
        .iter()
        .find(|(_, t)| *t == Target::Talk("acme-shop".into(), 3))
        .expect("Talk(3) hit");
    assert_eq!(buf[(0, sel_rect.y)].symbol(), "│");
    assert_eq!(buf[(sel_rect.x, sel_rect.y)].symbol(), "│");
    assert_eq!(buf[(sel_rect.x + 1, sel_rect.y)].symbol(), "▌");
    assert_eq!(buf[(sel_rect.x + 2, sel_rect.y)].symbol(), "✦");
    assert_eq!(buf[(sel_rect.x + 2, sel_rect.y)].fg, palette.accent);
    assert_eq!(buf[(sel_rect.x + 3, sel_rect.y)].bg, palette.selection_bg);
    assert_eq!(buf[(area.width - 1, sel_rect.y)].symbol(), "│");

    // Footer shows `⏎ read` when `file.is_some()`.
    let footer_with_file = rendered.last().unwrap();
    assert!(footer_with_file.contains("⏎ read"), "{footer_with_file}");

    // Selecting Talk(#4) (`file: None`) omits `⏎ read` from the footer.
    panel.selected = Some(Target::Talk("acme-shop".into(), 4));
    let mut buf2 = Buffer::empty(area);
    render(
        &mut buf2,
        area,
        &machines(Some(&summary)),
        |_| true,
        |_| true,
        &panel,
        PanelMode::Expanded,
        Some(&current),
        Some(&current),
        0,
        None,
        &StudioKeys::default(),
        None,
        false,
        false,
        true,
        &Default::default(),
        &palette,
        1,
    );
    let rendered2 = lines(&buf2, area);
    let footer_no_file = rendered2.last().unwrap();
    assert!(!footer_no_file.contains("⏎ read"), "{footer_no_file}");
    assert!(footer_no_file.contains("i tell"), "{footer_no_file}");
}

#[test]
fn talk_budget_degrades_gracefully_without_starving_roles_or_input() {
    let summary = summary_with_exchange();
    let panel = Panel {
        mode: PanelMode::Expanded,
        ..Panel::new()
    };
    let current = crate::studio::StudioKey::local("acme-shop");
    let talk_seqs = |body_height: u16| -> Vec<u64> {
        rows(
            &machines(Some(&summary)),
            |_| true,
            |_| true,
            &panel,
            PanelMode::Expanded,
            Some(&current),
            body_height,
        )
        .iter()
        .filter_map(|r| match r.target() {
            Target::Talk(_, seq) => Some(seq),
            _ => None,
        })
        .collect()
    };

    // Base block height in `summary()` is 4 (top + input-divider + input + bottom) + 8 roles = 12.
    // Unconstrained (0) and roomy (21 >= 12 + 6): all 6 turns render.
    assert_eq!(talk_seqs(0), vec![1, 2, 3, 4, 5, 6]);
    assert_eq!(talk_seqs(21), vec![1, 2, 3, 4, 5, 6]);
    // Partial slack (14 = 12 + 2): newest 2 turns render above `›`.
    assert_eq!(talk_seqs(14), vec![5, 6]);
    // Exact fit (12) or cramped (9): 0 turns render so roles + `›` never starve.
    assert_eq!(talk_seqs(12), Vec::<u64>::new());
    assert_eq!(talk_seqs(9), Vec::<u64>::new());

    // Even at exact fit (body_height = 12), every role and `›` Input remain present.
    let exact_targets: Vec<Target> = rows(
        &machines(Some(&summary)),
        |_| true,
        |_| true,
        &panel,
        PanelMode::Expanded,
        Some(&current),
        12,
    )
    .iter()
    .map(|r| r.target())
    .collect();
    assert_eq!(
        exact_targets
            .iter()
            .filter(|t| matches!(t, Target::Person(..)))
            .count(),
        7
    );
    assert!(exact_targets.contains(&Target::ClosedFold("acme-shop".into())));
    assert!(exact_targets.contains(&Target::Input("acme-shop".into())));
}

#[test]
fn studio_input_row_renders_queued_and_thinking_cues_and_clears_on_delivery() {
    let palette = Palette::from_name("catppuccin").unwrap();
    let area = Rect::new(0, 0, 38, 22);
    let panel = Panel {
        mode: PanelMode::Expanded,
        ..Panel::new()
    };
    let current = crate::studio::StudioKey::local("acme-shop");

    // 1. `pending_tell: true` -> `queued` badge on `›` row and `working_drawn = true`.
    let mut v = summary_json();
    for r in v["studios"][0]["roles"].as_array_mut().unwrap() {
        r["work"] = serde_json::Value::Null;
    }
    v["studios"][0]["pending_tell"] = serde_json::json!(true);
    v["studios"][0]["exchange"] = serde_json::json!([
        { "seq": 1, "who": "user", "text": "check status", "file": "messages/1.txt" }
    ]);
    let s_queued: Summary = serde_json::from_value(v.clone()).unwrap();
    let mut buf = Buffer::empty(area);
    let hits = render(
        &mut buf,
        area,
        &machines(Some(&s_queued)),
        |_| true,
        |_| true,
        &panel,
        PanelMode::Expanded,
        Some(&current),
        Some(&current),
        0,
        None,
        &StudioKeys::default(),
        None,
        false,
        false,
        true,
        &Default::default(),
        &palette,
        1,
    );
    assert!(hits.working_drawn, "queued cue arms spinner");
    let text_queued = lines(&buf, area).join("\n");
    assert!(text_queued.contains("queued"), "{text_queued}");

    // 2. Delivered (`pending_tell: false`) while producer is `working` -> `thinking` cue.
    v["studios"][0]["pending_tell"] = serde_json::json!(false);
    v["studios"][0]["roles"][0]["work"] = serde_json::json!("working");
    let s_thinking: Summary = serde_json::from_value(v.clone()).unwrap();
    let mut buf2 = Buffer::empty(area);
    let hits2 = render(
        &mut buf2,
        area,
        &machines(Some(&s_thinking)),
        |_| true,
        |_| true,
        &panel,
        PanelMode::Expanded,
        Some(&current),
        Some(&current),
        0,
        None,
        &StudioKeys::default(),
        None,
        false,
        false,
        true,
        &Default::default(),
        &palette,
        1,
    );
    assert!(hits2.working_drawn);
    let text_thinking = lines(&buf2, area).join("\n");
    assert!(!text_thinking.contains("queued"), "{text_thinking}");
    assert!(text_thinking.contains("thinking"), "{text_thinking}");

    // 3. Once producer replies (`exchange.last().who == "producer"`) and settles -> no cue.
    v["studios"][0]["roles"][0]["work"] = serde_json::json!("waiting");
    v["studios"][0]["exchange"].as_array_mut().unwrap().push(
        serde_json::json!({ "seq": 2, "who": "producer", "text": "all clear", "file": "messages/2.md" }),
    );
    let s_done: Summary = serde_json::from_value(v).unwrap();
    let mut buf3 = Buffer::empty(area);
    let hits3 = render(
        &mut buf3,
        area,
        &machines(Some(&s_done)),
        |_| true,
        |_| true,
        &panel,
        PanelMode::Expanded,
        Some(&current),
        Some(&current),
        0,
        None,
        &StudioKeys::default(),
        None,
        false,
        false,
        true,
        &Default::default(),
        &palette,
        1,
    );
    assert!(!hits3.working_drawn, "settled studio stops animating");
    let text_done = lines(&buf3, area).join("\n");
    assert!(!text_done.contains("queued"), "{text_done}");
    assert!(!text_done.contains("thinking"), "{text_done}");
}
