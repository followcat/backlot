//! The bar's model: the frames and the band rows — all pure reads
//! of the document.
use super::*;
use crate::hollywood::rows::{Frame, Live, Row, Target, band_rows, frames, hollywood_live, on_set};
use crate::hollywood::sheet::{Conn, Status, Wait};
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
    // acme-shop's screen is stalled (stuck, not the user's), so the
    // sheet lists nothing and the calm row names all three studios.
    assert!(matches!(rows[1], Row::Calm { studios: 3 }));
}

/// A blocked Hollywood is on the needs-you list — the bar is never
/// calm while it waits on a person.
#[test]
fn a_blocked_hollywood_is_not_calm() {
    let mut value = summary_json();
    value["hollywood"]["cards"] = serde_json::json!([]);
    value["hollywood"]["work"] = serde_json::json!("blocked");
    let summary: Summary = serde_json::from_value(value).unwrap();
    let list = machines(Some(&summary));
    let (waits, rows) = band_rows(&list, &all_live, true);
    let hollywood = waits
        .iter()
        .find(|w| matches!(w, Wait::Hollywood(_)))
        .expect("Hollywood is listed");
    assert_eq!(
        crate::hollywood::items::wait_head(hollywood, "Local", false).0,
        "HOLLYWOOD blocked"
    );
    assert!(!rows.iter().any(|r| matches!(r, Row::Calm { .. })));
}

/// A Hollywood permission prompt is the asks row: the same `y` allow
/// once / `d` deny / `p` peek / Enter the #109 live row takes, on the
/// same target.
#[test]
fn a_hollywood_prompt_is_an_asks_row() {
    let mut value = summary_json();
    value["hollywood"]["cards"] = serde_json::json!([]);
    value["hollywood"]["work"] = serde_json::json!("blocked");
    value["hollywood"]["attention_user"] = serde_json::json!(true);
    value["hollywood"]["attention"] = serde_json::json!("permission prompt");
    value["hollywood"]["attention_detail"] = serde_json::json!("may I run cargo?");
    let summary: Summary = serde_json::from_value(value).unwrap();
    let list = machines(Some(&summary));
    let (waits, _) = band_rows(&list, &all_live, true);
    let hollywood = waits
        .iter()
        .find(|w| matches!(w, Wait::Hollywood(_)))
        .expect("Hollywood is listed");
    assert_eq!(
        crate::hollywood::items::wait_head(hollywood, "Local", false),
        ("HOLLYWOOD asks: may I run cargo?".into(), "⚠")
    );
    assert_eq!(hollywood.target(), Target::Hollywood);
    let keys: Vec<&str> = crate::hollywood::items::chips_of(hollywood)
        .iter()
        .map(|c| c.key)
        .collect();
    assert_eq!(keys, ["y", "d", "p", "↵"]);
}

/// A prompt on a runner that cannot answer offers only peek and
/// Enter — no `allow y`, no `deny d` — and its detail says where to
/// answer.
#[test]
fn a_prompt_the_runner_cannot_answer_names_no_y_or_d() {
    let mut value = summary_json();
    value["hollywood"]["cards"] = serde_json::json!([]);
    let mut summary: Summary = serde_json::from_value(value).unwrap();
    let role = summary.studios[0]
        .roles
        .iter_mut()
        .find(|r| r.role == "actor-alice")
        .expect("alice");
    role.attention_user = true;
    role.attention_seq = Some(7);
    role.attention_detail = Some("may I run the tests?".into());
    role.can_answer_prompt = false;
    let list = machines(Some(&summary));
    let (waits, _) = band_rows(&list, &all_live, true);
    let prompt = waits
        .iter()
        .find(|w| matches!(w, Wait::Prompt(_)))
        .expect("the prompt row");
    assert!(!prompt.can_answer());
    let keys: Vec<&str> = crate::hollywood::items::chips_of(prompt)
        .iter()
        .map(|c| c.key)
        .collect();
    assert_eq!(keys, ["p", "↵"]);
    assert_eq!(
        crate::hollywood::items::detail_text(prompt).as_deref(),
        Some("may I run the tests? · answer in its terminal (Enter)")
    );
}

/// A stage-bookkeeping attention alone leaves the bar calm: the
/// person can do nothing about it.
#[test]
fn an_info_attention_alone_stays_calm() {
    let mut value = summary_json();
    value["hollywood"]["cards"] = serde_json::json!([]);
    value["studios"][0]["roles"][1]["attention"] = serde_json::json!("session id not found");
    value["studios"][0]["roles"][1]["attention_class"] = serde_json::json!("info");
    let summary: Summary = serde_json::from_value(value).unwrap();
    let list = machines(Some(&summary));
    let (waits, rows) = band_rows(&list, &all_live, true);
    assert!(
        waits.is_empty(),
        "{:?}",
        waits.iter().map(|w| w.target()).collect::<Vec<_>>()
    );
    assert!(matches!(rows[1], Row::Calm { studios: 3 }));
}

/// A studio with no requirement is listed with its next step.
#[test]
fn a_studio_with_no_requirement_is_listed_with_its_next_step() {
    let mut value = summary_json();
    value["hollywood"]["cards"] = serde_json::json!([]);
    value["studios"][0]["requirement_version"] = serde_json::json!(0);
    let summary: Summary = serde_json::from_value(value).unwrap();
    let list = machines(Some(&summary));
    let (waits, _) = band_rows(&list, &all_live, true);
    let found = waits
        .iter()
        .find(|w| matches!(w, Wait::NoRequirement(_)))
        .expect("the new studio lists");
    assert_eq!(
        found.target(),
        Target::Tell(crate::studio::StudioKey::local("acme-shop"))
    );
    assert_eq!(
        crate::hollywood::items::detail_text(found).as_deref(),
        Some("next: tell the producer what to build")
    );
    let Wait::NoRequirement(item) = found else {
        unreachable!()
    };
    assert_eq!(item.studio.id, "acme-shop");
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
            Person::Stuck,   // alice, a stalled screen
            Person::Lost,    // bob, lost
            Person::Ready,   // carol, live and nothing assigned
        ]
    );
}

/// R3-M1: the headline row never doubles the newest Hollywood
/// exchange reply — the same seq or the same text means the band
/// shows the report once.
#[test]
fn the_headline_never_repeats_the_newest_reply() {
    // Baseline: the fixture has a headline and no exchange — the
    // headline row lists.
    let summary = summary();
    let list = machines(Some(&summary));
    let (_, rows) = band_rows(&list, &all_live, true);
    assert!(matches!(rows[0], Row::Headline("acme-shop needs a look")));

    // The newest reply already carries the headline's text — no
    // second copy above the waits.
    let mut value = summary_json();
    value["hollywood"]["exchange"] = serde_json::json!([
        { "who": "user", "seq": 8, "text": "how is the shop?" },
        { "who": "hollywood", "seq": 9, "text": "acme-shop needs a look" },
    ]);
    let summary: Summary = serde_json::from_value(value).unwrap();
    let list = machines(Some(&summary));
    let (_, rows) = band_rows(&list, &all_live, true);
    assert!(
        rows.iter().all(|r| !matches!(r, Row::Headline(_))),
        "the headline must dedup"
    );
    assert_eq!(rows.iter().filter(|r| matches!(r, Row::Talk(_))).count(), 2);

    // The seq tie counts even when the texts differ — `headline_seq`
    // names the reply it repeats.
    let mut value = summary_json();
    value["hollywood"]["headline_seq"] = serde_json::json!(9);
    value["hollywood"]["exchange"] = serde_json::json!([
        { "who": "hollywood", "seq": 9, "text": "a longer report follows" },
    ]);
    let summary: Summary = serde_json::from_value(value).unwrap();
    let list = machines(Some(&summary));
    let (_, rows) = band_rows(&list, &all_live, true);
    assert!(
        rows.iter().all(|r| !matches!(r, Row::Headline(_))),
        "the headline must dedup"
    );

    // A headline that is not the newest reply still lists — older
    // replies are not it.
    let mut value = summary_json();
    value["hollywood"]["headline_seq"] = serde_json::json!(7);
    value["hollywood"]["exchange"] = serde_json::json!([
        { "who": "hollywood", "seq": 9, "text": "something else entirely" },
    ]);
    let summary: Summary = serde_json::from_value(value).unwrap();
    let list = machines(Some(&summary));
    let (_, rows) = band_rows(&list, &all_live, true);
    assert!(matches!(rows[0], Row::Headline("acme-shop needs a look")));
}

/// A document with one unanswered `user ›` line, its Hollywood state
/// set to `patch`.
fn with_live(patch: serde_json::Value) -> Summary {
    let mut value = summary_json();
    value["hollywood"]["exchange"] =
        serde_json::json!([{ "seq": 8, "who": "user", "text": "recast the producer" }]);
    for (key, setting) in patch.as_object().unwrap() {
        value["hollywood"][key] = setting.clone();
    }
    serde_json::from_value(value).unwrap()
}

/// The live row sits under the newest user line — pending first, then
/// working (with the terminal's `doing`), then a user-owned prompt —
/// and no row at all once a reply follows.
#[test]
fn the_live_row_follows_the_newest_user_line_until_a_reply() {
    // Pending: the line is still behind the send gate.
    let summary = with_live(serde_json::json!({ "pending": true }));
    let list = machines(Some(&summary));
    let (waits, rows) = band_rows(&list, &all_live, true);
    let live = rows
        .iter()
        .position(|r| matches!(r, Row::Live(_)))
        .expect("the live row");
    assert!(matches!(rows[live], Row::Live(Live::Pending)));
    assert!(
        matches!(rows[live - 1], Row::Talk(line) if line.who == "user"),
        "the live row follows the newest user line"
    );
    assert_eq!(rows[live].target(&waits), Target::Hollywood);

    // Working: the terminal's doing, else the bare word.
    let summary = with_live(serde_json::json!({
        "work": "working",
        "doing": "reading the ledgers",
    }));
    let list = machines(Some(&summary));
    let (_, rows) = band_rows(&list, &all_live, true);
    assert!(
        rows.iter()
            .any(|r| matches!(r, Row::Live(Live::Working(Some("reading the ledgers")))))
    );
    let summary = with_live(serde_json::json!({ "work": "working" }));
    let list = machines(Some(&summary));
    let (_, rows) = band_rows(&list, &all_live, true);
    assert!(
        rows.iter()
            .any(|r| matches!(r, Row::Live(Live::Working(None))))
    );

    // A user-owned prompt wins over both.
    let summary = with_live(serde_json::json!({
        "pending": true,
        "work": "blocked",
        "attention_user": true,
        "attention_detail": "backlot --help",
    }));
    let list = machines(Some(&summary));
    let (_, rows) = band_rows(&list, &all_live, true);
    assert!(
        rows.iter()
            .any(|r| matches!(r, Row::Live(Live::Prompt("backlot --help"))))
    );

    // A reply after the user line ends the row.
    let mut value = summary_json();
    value["hollywood"]["exchange"] = serde_json::json!([
        { "seq": 8, "who": "user", "text": "recast the producer" },
        { "seq": 9, "who": "hollywood", "text": "done" },
    ]);
    value["hollywood"]["work"] = "working".into();
    let summary: Summary = serde_json::from_value(value).unwrap();
    let list = machines(Some(&summary));
    let (_, rows) = band_rows(&list, &all_live, true);
    assert!(!rows.iter().any(|r| matches!(r, Row::Live(_))));
    assert!(hollywood_live(&list).is_none());
}

/// Only a user-owned prompt lights the row's `y`/`d`; every other
/// state leaves them to the footer's silence.
#[test]
fn hollywood_asking_tracks_the_user_owned_prompt() {
    let list = machines(Some(&with_live(serde_json::json!({
        "work": "working",
        "doing": "reading the ledgers",
    }))));
    assert!(!crate::hollywood::hollywood_asking(&list));
    let list = machines(Some(&with_live(serde_json::json!({
        "work": "blocked",
        "attention_user": true,
        "attention_detail": "backlot --help",
    }))));
    assert!(crate::hollywood::hollywood_asking(&list));
}
