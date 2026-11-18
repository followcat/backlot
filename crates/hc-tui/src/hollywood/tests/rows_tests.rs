//! The bar's model: the frames and the band rows.
//! and the cast marks — all pure reads of the document.
use super::*;
use crate::hollywood::rows::{Frame, Row, Target, band_rows, frames, on_set};
use crate::studio::glyphs::Person;
use backlot_model::summary::Summary;

#[test]
fn the_frames_are_the_studios_then_the_new_studio_entry() {
    let summary = summary();
    let list = machines(Some(&summary));
    let all = frames(&list);
    assert_eq!(all.len(), 4);
    assert_eq!(
        all.iter().map(|f| f.target()).collect::<Vec<_>>(),
        vec![
            Target::Studio("acme-shop".into()),
            Target::Studio("beta-tools".into()),
            Target::Studio("gamma-site".into()),
            Target::NewStudio,
        ]
    );
    // The last frame is always the entry.
    assert!(matches!(all.last(), Some(Frame::New)));
}

#[test]
fn a_missing_document_still_offers_the_entry() {
    let all = frames(&[]);
    assert_eq!(all.len(), 1);
    assert!(matches!(all[0], Frame::New));
}

#[test]
fn the_band_is_the_headline_the_cards_and_the_input() {
    let summary = summary();
    let all = band_rows(Some(&summary), true, true);
    let targets: Vec<Target> = all.iter().map(|r| r.target()).collect();
    assert_eq!(
        targets,
        vec![
            Target::Headline,
            Target::Card("c3".into()),
            Target::Card("c4".into()),
            Target::Input,
        ]
    );
}

#[test]
fn a_dead_stage_leads_the_band() {
    let summary = summary();
    let all = band_rows(Some(&summary), false, true);
    assert!(matches!(all[0], Row::StageDown));
    assert_eq!(all[0].target(), Target::StageDown);
}

#[test]
fn without_an_agent_the_band_is_the_setup_line() {
    let summary = summary();
    let all = band_rows(Some(&summary), true, false);
    assert_eq!(all.len(), 1);
    assert!(matches!(all[0], Row::Setup));
    assert_eq!(frames(&machines(Some(&summary))).len(), 4);
}

#[test]
fn a_document_without_a_hollywood_object_has_a_setup_free_band() {
    // No agent: the studios still show as frames; the band is the
    // setup line and nothing else.
    let mut value = summary_json();
    value.as_object_mut().unwrap().remove("hollywood");
    let summary: Summary = serde_json::from_value(value).unwrap();
    let all = band_rows(Some(&summary), true, false);
    assert_eq!(all.len(), 1);
    assert!(matches!(all[0], Row::Setup));
    assert_eq!(frames(&machines(Some(&summary))).len(), 4);
}

#[test]
fn the_role_on_set_is_the_first_working_one_else_the_producer() {
    let summary = summary();
    let acme = &summary.studios[0];
    let who = on_set(acme).expect("a role on set");
    assert_eq!(who.name, "producer");
    // A studio with only idle roles still names the producer.
    let beta = &summary.studios[1];
    assert_eq!(on_set(beta).map(|r| r.name.as_str()), Some("producer"));
    // No roles at all: nothing on set.
    assert!(on_set(&summary.studios[2]).is_none());
}

#[test]
fn cast_marks_follow_the_role_state() {
    let summary = summary();
    let marks: Vec<Person> = summary.studios[0].roles.iter().map(Person::of).collect();
    assert_eq!(
        marks,
        vec![
            Person::Working, // producer, working
            Person::Stalled, // alice, an attention
            Person::Lost,    // bob, lost
            Person::Idle,    // carol, waiting
        ]
    );
}
