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
    assert_eq!(rows[live].target(&waits), Target::HollywoodLive);

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

/// A card Hollywood proposed after the person's line reads as a talk
/// line under it — the same `who: hollywood` side a report takes —
/// and ends the live row, so the person sees the answer where they
/// spoke.
#[test]
fn a_card_after_the_user_line_is_a_talk_line_and_ends_the_live_row() {
    let mut value = summary_json();
    value["hollywood"]["exchange"] = serde_json::json!([
        { "seq": 8, "who": "user", "text": "recast the producer" },
        { "seq": 9, "who": "hollywood", "kind": "card",
          "text": "proposed card c3 · acme-shop · pause t1" },
    ]);
    value["hollywood"]["work"] = "working".into();
    let summary: Summary = serde_json::from_value(value).unwrap();
    let list = machines(Some(&summary));
    let (waits, rows) = band_rows(&list, &all_live, true);
    let talk: Vec<(&str, Option<&str>)> = rows
        .iter()
        .filter_map(|r| match r {
            Row::Talk(line) => Some((line.who.as_str(), line.kind.as_deref())),
            _ => None,
        })
        .collect();
    assert_eq!(
        talk,
        [("user", None), ("hollywood", Some("card"))],
        "the card is Hollywood's line"
    );
    assert!(
        !rows.iter().any(|r| matches!(r, Row::Live(_))),
        "a card ends the live row like a report"
    );
    assert!(hollywood_live(&list).is_none());
    // Enter on the card line opens its entry, keyed by the ledger seq.
    let card = rows
        .iter()
        .find_map(|r| match r {
            Row::Talk(line) if line.kind.as_deref() == Some("card") => Some(*line),
            _ => None,
        })
        .expect("the card line");
    assert_eq!(
        Row::Talk(card).target(&waits),
        Target::Talk(9),
        "the card line keeps the ledger's identity"
    );
}

/// An idle Hollywood with an unanswered `user ›` line shows the live
/// row's idle word — a reply that never came, not silence.
#[test]
fn an_idle_hollywood_says_it_has_not_answered() {
    let summary = with_live(serde_json::json!({ "work": "waiting" }));
    let list = machines(Some(&summary));
    let (_, rows) = band_rows(&list, &all_live, true);
    assert!(
        rows.iter().any(|r| matches!(r, Row::Live(Live::Idle))),
        "the idle live row: {:?}",
        rows.iter()
            .filter_map(|r| match r {
                Row::Live(live) => Some(*live),
                _ => None,
            })
            .collect::<Vec<_>>()
    );
    assert_eq!(
        crate::hollywood::items::live_head(&Live::Idle),
        (
            "hollywood is idle and has not answered · open ↵".into(),
            "○"
        )
    );
}

/// The talk keeps the newest turn and the turn before it; older lines
/// fold into one row. Enter on the fold opens the newest folded entry.
#[test]
fn older_turns_fold_into_one_row() {
    let mut value = summary_json();
    value["hollywood"]["exchange"] = serde_json::json!([
        { "seq": 1, "who": "user", "text": "first ask" },
        { "seq": 2, "who": "hollywood", "text": "first answer" },
        { "seq": 3, "who": "user", "text": "second ask" },
        { "seq": 4, "who": "hollywood", "text": "second answer" },
        { "seq": 5, "who": "user", "text": "third ask" },
        { "seq": 6, "who": "hollywood", "text": "third answer" },
    ]);
    let summary: Summary = serde_json::from_value(value).unwrap();
    let list = machines(Some(&summary));
    let (waits, rows) = band_rows(&list, &all_live, true);
    // The newest turn (#5, #6) and the one before (#3, #4) stay; the
    // two oldest lines (#1, #2) fold.
    let fold = rows
        .iter()
        .position(|r| matches!(r, Row::Fold { .. }))
        .expect("a fold row");
    assert!(
        matches!(
            rows[fold],
            Row::Fold {
                count: 2,
                newest: 2
            }
        ),
        "{:?}",
        rows.iter().map(|r| r.target(&waits)).collect::<Vec<_>>()
    );
    let talk: Vec<u64> = rows
        .iter()
        .filter_map(|r| match r {
            Row::Talk(line) => Some(line.seq),
            _ => None,
        })
        .collect();
    assert_eq!(talk, [3, 4, 5, 6]);
    // Enter on the fold opens the newest folded entry (#2).
    assert_eq!(rows[fold].target(&waits), Target::Talk(2));
}

/// With no user line at all, the newest two lines show and the rest
/// fold.
#[test]
fn without_a_user_line_the_newest_two_lines_show() {
    let mut value = summary_json();
    value["hollywood"]["exchange"] = serde_json::json!([
        { "seq": 1, "who": "hollywood", "text": "one" },
        { "seq": 2, "who": "hollywood", "text": "two" },
        { "seq": 3, "who": "hollywood", "text": "three" },
    ]);
    let summary: Summary = serde_json::from_value(value).unwrap();
    let list = machines(Some(&summary));
    let (waits, rows) = band_rows(&list, &all_live, true);
    let fold = rows
        .iter()
        .position(|r| matches!(r, Row::Fold { .. }))
        .expect("a fold row");
    assert!(matches!(
        rows[fold],
        Row::Fold {
            count: 1,
            newest: 1
        }
    ));
    let talk: Vec<u64> = rows
        .iter()
        .filter_map(|r| match r {
            Row::Talk(line) => Some(line.seq),
            _ => None,
        })
        .collect();
    assert_eq!(talk, [2, 3]);
    assert_eq!(rows[fold].target(&waits), Target::Talk(1));
}

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

/// One studio contributes one band row: its ask leads the card and
/// the folded decision count remains in the underlying waits list.
#[test]
fn one_studio_rows_asks_fold_the_rest() {
    let mut value = summary_json();
    value["studios"][0]["roles"][0]["asks"] =
        serde_json::json!("What should we build? Start small.");
    value["studios"][0]["exchange"] = serde_json::json!([
        { "seq": 10, "who": "producer", "to": "user", "kind": "tell", "ask": true,
          "ts_ms": 1, "text": "What should we build? Start small." }
    ]);
    let summary: Summary = serde_json::from_value(value).unwrap();
    let list = machines(Some(&summary));
    let (waits, rows) = band_rows(&list, &all_live, true);
    assert_eq!(waits.len(), 3, "ask plus both cards are decisions");
    let wait_rows: Vec<_> = rows
        .iter()
        .filter_map(|r| match r {
            Row::Wait(i) => Some(waits[*i].target()),
            _ => None,
        })
        .collect();
    assert_eq!(
        wait_rows,
        vec![
            Target::Role(
                crate::studio::StudioKey::local("acme-shop"),
                "producer".into()
            ),
            Target::Card("c4".into())
        ]
    );
    let acme = waits
        .iter()
        .find(|w| matches!(w, Wait::Role(r) if r.asks))
        .unwrap();
    assert_eq!(crate::hollywood::rows::group_extra(&waits, acme), 1);
}

/// READY leads its own studio even though asks sort earlier in the
/// flat urgency list, absorbing that studio's question row.
#[test]
fn ready_absorbs_the_studios_asks() {
    let mut value = summary_json();
    value["studios"][0]["ready_to_finish"] = serde_json::json!(true);
    value["studios"][0]["roles"][0]["asks"] = serde_json::json!("Ship it?");
    let summary: Summary = serde_json::from_value(value).unwrap();
    let list = machines(Some(&summary));
    let (waits, rows) = band_rows(&list, &all_live, true);
    let acme_rows: Vec<_> = rows
        .iter()
        .filter_map(|r| match r {
            Row::Wait(i) => Some(waits[*i].target()),
            _ => None,
        })
        .filter(|t| matches!(t, Target::Ready(k) if k.id == "acme-shop"))
        .collect();
    assert_eq!(acme_rows.len(), 1);
    assert!(!rows.iter().any(|r| matches!(
        r.target(&waits),
        Target::Role(k, _) if k.id == "acme-shop"
    )));
}

#[test]
fn a_studio_needing_upgrade_emits_a_single_line_wait_in_call_sheet() {
    let mut value = summary_json();
    value["studios"][0]["needs_upgrade"] = serde_json::json!(true);
    value["studios"][0]["unknown_kinds"] = serde_json::json!(["design.opened"]);
    value["studios"][0]["ready_to_finish"] = serde_json::json!(true);
    let summary: Summary = serde_json::from_value(value).unwrap();
    let list = machines(Some(&summary));
    let (waits, rows) = band_rows(&list, &all_live, true);
    let upgrade_waits: Vec<_> = waits
        .iter()
        .filter(|w| {
            crate::hollywood::rows::wait_studio_key(w).is_some_and(|(_, id)| id == "acme-shop")
                && !w.detailable()
                && matches!(w, Wait::Upgrade(_))
        })
        .collect();
    assert_eq!(upgrade_waits.len(), 1, "waits count = {}", waits.len());
    assert!(
        !waits
            .iter()
            .any(|w| matches!(w, Wait::Ready(r) if r.studio.id == "acme-shop")),
        "ready_to_finish must not emit Wait::Ready while studio needs upgrade"
    );
    let wait_rows: Vec<_> = rows
        .iter()
        .filter(|r| matches!(r, Row::Wait(i) if waits[*i].target() == upgrade_waits[0].target()))
        .collect();
    assert_eq!(wait_rows.len(), 1);
}
