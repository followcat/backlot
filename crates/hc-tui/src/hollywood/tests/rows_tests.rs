//! The bar's model: the frames and the band rows — all pure reads
//! of the document.
use super::*;
use crate::hollywood::rows::{Frame, Row, Target, band_rows, frames, on_set};
use crate::hollywood::sheet::{Conn, Status};
use crate::studio::glyphs::Person;
use backlot_model::summary::Summary;

/// Every machine online, every stage running.
fn all_live(_: Option<&str>) -> (Conn, bool) {
    (Conn::Online, true)
}

/// Every machine online, every stage down.
fn stage_down(_: Option<&str>) -> (Conn, bool) {
    (Conn::Online, false)
}

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
fn the_band_is_the_headline_the_waits_and_the_input() {
    let summary = summary();
    let list = machines(Some(&summary));
    let (waits, rows) = band_rows(&list, &all_live, true);
    // The fixture carries two open cards and no prompts — the waits
    // are the cards, in document order.
    assert_eq!(waits.len(), 2);
    let targets: Vec<Target> = rows.iter().map(|r| r.target(&waits)).collect();
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
fn a_dead_stage_leads_the_waits_when_it_has_live_studios() {
    let summary = summary();
    let list = machines(Some(&summary));
    let down: &Status<'_> = &stage_down;
    let (waits, _) = band_rows(&list, down, true);
    assert_eq!(waits[0].target(), Target::Stage(None, false));
}

#[test]
fn without_an_agent_the_band_is_the_setup_line() {
    let summary = summary();
    let list = machines(Some(&summary));
    let (waits, rows) = band_rows(&list, &all_live, false);
    // Cards stay listed without an agent — they are still the
    // user's; only the input line becomes the setup line.
    assert_eq!(rows.len(), 3);
    assert!(matches!(rows.last(), Some(Row::Setup)));
    assert!(!waits.is_empty());
    assert_eq!(frames(&list).len(), 4);
}

#[test]
fn a_calm_band_still_ends_in_the_input_line() {
    let mut value = summary_json();
    value["hollywood"]["cards"] = serde_json::json!([]);
    let summary: Summary = serde_json::from_value(value).unwrap();
    let list = machines(Some(&summary));
    let (waits, rows) = band_rows(&list, &all_live, true);
    assert!(waits.is_empty());
    assert_eq!(rows.len(), 3); // headline, calm, input
    assert!(matches!(rows[1], Row::Calm(3)));
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
