//! The layer's row model and the strip's one-line content.
use super::*;
use crate::hollywood::rows::Row;
use crate::hollywood::{Target, rows, strip_text};

#[test]
fn expanded_rows_are_title_body_then_input() {
    let summary = summary();
    let all = rows(Some(&summary), true, true);
    assert!(matches!(all[0], Row::Title));
    assert!(matches!(all.last(), Some(Row::Input)));
    let body: Vec<Target> = all[1..all.len() - 1].iter().map(|r| r.target()).collect();
    // Studios first, then the headline, then the open cards.
    assert_eq!(
        body,
        vec![
            Target::Studio("acme-shop".into()),
            Target::Studio("beta-tools".into()),
            Target::Headline,
            Target::Card("c3".into()),
            Target::Card("c4".into()),
        ]
    );
}

#[test]
fn a_dead_stage_adds_its_line_and_keeps_the_studios() {
    let summary = summary();
    let all = rows(Some(&summary), false, true);
    assert!(matches!(all[1], Row::StageDown));
    assert!(
        all.iter()
            .any(|r| matches!(r, Row::Studio(s) if s.id == "acme-shop"))
    );
}

#[test]
fn no_document_and_no_hollywood_section_degrade() {
    // No document at all.
    let all = rows(None, true, true);
    assert!(matches!(all[1], Row::Empty));
    assert!(matches!(all.last(), Some(Row::Input)));
    // A document with no `hollywood` object: studios still show,
    // no headline or cards appear.
    let mut value = summary_json();
    value.as_object_mut().unwrap().remove("hollywood");
    let summary: Summary = serde_json::from_value(value).unwrap();
    let all = rows(Some(&summary), true, true);
    assert!(all.iter().any(|r| matches!(r, Row::Studio(_))));
    assert!(!all.iter().any(|r| matches!(r, Row::Headline(_))));
    assert!(!all.iter().any(|r| matches!(r, Row::Card(_))));
}

#[test]
fn the_strip_names_state_counts_cards_and_headline() {
    let summary = summary();
    let text = strip_text(Some(&summary), true);
    assert!(text.contains("✦ hollywood"), "{text}");
    assert!(text.contains("live"), "{text}");
    assert!(text.contains("1 live"), "{text}");
    assert!(text.contains("1 fin"), "{text}");
    assert!(text.contains("2 cards"), "{text}");
    assert!(text.contains("acme-shop needs a look"), "{text}");
}

#[test]
fn the_strip_says_down_closed_and_lost() {
    // No hollywood object → the role is down.
    let mut value = summary_json();
    value.as_object_mut().unwrap().remove("hollywood");
    let summary: Summary = serde_json::from_value(value.clone()).unwrap();
    assert!(strip_text(Some(&summary), true).contains("down"));
    for state in ["closed", "lost"] {
        value["hollywood"]["state"] = serde_json::json!(state);
        let summary: Summary = serde_json::from_value(value.clone()).unwrap();
        assert!(strip_text(Some(&summary), true).contains(state), "{state}");
    }
}

#[test]
fn the_strip_counts_cards_only_while_open() {
    let mut value = summary_json();
    value["hollywood"]["cards"] = serde_json::json!([]);
    let summary: Summary = serde_json::from_value(value.clone()).unwrap();
    assert!(!strip_text(Some(&summary), true).contains("card"));
    value["hollywood"]["cards"] = serde_json::json!([{
        "id": "c1", "studio": "acme-shop", "action": "pause t1", "why": "w"
    }]);
    let summary: Summary = serde_json::from_value(value).unwrap();
    assert!(strip_text(Some(&summary), true).contains("1 card"));
}

#[test]
fn without_an_agent_the_last_row_is_the_setup_line() {
    let summary = summary();
    let all = rows(Some(&summary), true, false);
    assert!(matches!(all.last(), Some(Row::Setup)));
    assert!(
        !all.iter()
            .any(|r| matches!(r, Row::Headline(_) | Row::Card(_))),
        "the agent's rows need the agent"
    );
    assert!(all.iter().any(|r| matches!(r, Row::Studio(_))));
    let text = strip_text(Some(&summary), false);
    assert!(text.contains("2 studios"), "{text}");
    assert!(!text.contains("card"), "{text}");
}
