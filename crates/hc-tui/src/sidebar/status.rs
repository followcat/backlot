//! Per-terminal status classification. The only authority is what core
//! reports in the record — `status`/`records` (OSC 7501), `state`,
//! `pid`, `reason` — never screen text, argv or foreground names.
//! Semantics are written down in docs/sidebar.md.

use super::text;
use hc_proto::terminal::{Status, Terminal};
use serde_json::Value;

/// The program's lifecycle, exactly as core's `state`/`pid`/`reason`
/// contract defines it.
#[derive(Debug, Clone, PartialEq)]
pub enum Life {
    /// `state` is not `live`: the program ended (`exit_code`/`signal`/
    /// `reason` say how).
    Finished,
    /// Live record without a pid and with a real reason (`holder_lost`,
    /// `host_shutdown`, ...): the program died out from under the
    /// terminal — not the same as finished.
    Lost,
    /// Live record without a pid and no real reason (`spawn_pending`
    /// counts as starting): the holder may still be bringing it up.
    Pending,
    /// `state` is `live` and a pid exists.
    Live,
    /// A newer lifecycle word core added that this build doesn't know —
    /// kept verbatim, never folded into finished.
    Other(String),
}

/// One reported status word.
#[derive(Debug, Clone, PartialEq)]
pub enum Word {
    /// Needs the person (`kind`: permission/question/auth when given).
    Blocked { kind: Option<String> },
    /// The program reported `error`.
    Error,
    /// `working` (`progress`: 0..100 when given).
    Working { progress: Option<u8> },
    /// Finished a turn — ready for the next.
    Done,
    /// Quiet at a prompt.
    Idle,
    /// A newer or unfamiliar word, kept verbatim — never relabeled.
    Other(String),
}

/// The winning status record for one terminal.
#[derive(Debug, Clone, PartialEq)]
pub struct Effective {
    /// Its word.
    pub word: Word,
    /// Which named record won; `None` is the root record.
    pub record: Option<String>,
    /// A patched core's `source` extension, verbatim (`screen`, ...).
    pub source: Option<String>,
}

/// A terminal reduced to what the sidebar needs.
#[derive(Debug, Clone, PartialEq)]
pub struct TermStatus {
    /// Program lifecycle.
    pub life: Life,
    /// Winning status record, when one was ever reported.
    pub effective: Option<Effective>,
    /// Whether the terminal counts as an agent (see docs/sidebar.md).
    pub agent: bool,
    /// Aggregation rank: higher needs attention sooner.
    pub urgency: u8,
    /// The badge text for the row, already sanitized.
    pub badge: String,
}

impl Word {
    /// Urgency within live reports (docs/sidebar.md has the full order).
    fn rank(&self) -> u8 {
        match self {
            Word::Blocked { .. } => 70,
            Word::Error => 60,
            Word::Working { .. } => 40,
            Word::Other(_) => 30,
            Word::Done => 20,
            Word::Idle => 10,
        }
    }

    /// The word text for one record (`kind`/`progress` folded in).
    fn text(&self) -> String {
        match self {
            Word::Blocked { kind } => match kind {
                Some(kind) => format!("blocked·{}", text::clean(kind)),
                None => "blocked".into(),
            },
            Word::Error => "error".into(),
            Word::Working { progress } => match progress {
                Some(p) => format!("working {p}%"),
                None => "working".into(),
            },
            Word::Done => "done".into(),
            Word::Idle => "idle".into(),
            Word::Other(word) => text::clean(word),
        }
    }
}

fn word_of(status: &Status) -> Word {
    let kind = || status.kind.clone().filter(|k| !k.is_empty());
    match status.state.as_str() {
        "blocked" => Word::Blocked { kind: kind() },
        "error" => Word::Error,
        "working" => Word::Working {
            progress: status.progress,
        },
        "done" => Word::Done,
        "idle" => Word::Idle,
        other => Word::Other(other.into()),
    }
}

/// A patched core may tag a record's origin in `extra["source"]`
/// (e.g. screen-detected vs OSC 7501); kept verbatim, never interpreted.
fn source_of(status: &Status) -> Option<String> {
    status
        .extra
        .get("source")
        .and_then(Value::as_str)
        .map(str::to_string)
}

fn classify(term: &Terminal) -> Life {
    match term.state.as_str() {
        "finished" => Life::Finished,
        "live" => match (term.pid.is_none(), term.reason.as_deref()) {
            (true, None) | (true, Some("spawn_pending")) => Life::Pending,
            (true, Some(_)) => Life::Lost,
            (false, _) => Life::Live,
        },
        // Newer lifecycle words stay verbatim — never "finished".
        other => Life::Other(other.into()),
    }
}

/// The effective record: the most urgent over the root record and all
/// named records; ties prefer the root, then records by name (the map
/// is already sorted).
fn effective(term: &Terminal) -> Option<Effective> {
    let mut best: Option<(u8, Effective)> = None;
    let mut offer = |record: Option<String>, status: &Status| {
        let word = word_of(status);
        let rank = word.rank();
        let wins = best.as_ref().is_none_or(|(r, _)| rank > *r);
        if wins {
            best = Some((
                rank,
                Effective {
                    word,
                    record,
                    source: source_of(status),
                },
            ));
        }
    };
    if let Some(status) = &term.status {
        offer(None, status);
    }
    for (name, status) in &term.records {
        offer(Some(name.clone()), status);
    }
    best.map(|(_, e)| e)
}

/// Badge text for the row: the reported word always leads — a named
/// record trails as `word·name`, extensions as `·source` — so clipping
/// can only ever eat the optional detail, never the word itself. For
/// terminals without a report the lifecycle word is the badge.
fn badge(term: &Terminal, life: &Life, effective: Option<&Effective>) -> String {
    match (life, effective) {
        (Life::Finished, _) => {
            let mut s = "finished".to_string();
            if let Some(code) = term.exit_code {
                s.push_str(&format!(":{code}"));
            }
            if let Some(reason) = &term.reason {
                s.push_str(&format!("·{}", text::clean(reason)));
            }
            s
        }
        (Life::Lost, _) => match &term.reason {
            Some(reason) => format!("lost·{}", text::clean(reason)),
            None => "lost".into(),
        },
        (Life::Pending, _) => "starting".into(),
        (Life::Other(word), _) => match &term.reason {
            Some(reason) => format!("{}·{}", text::clean(word), text::clean(reason)),
            None => text::clean(word),
        },
        (Life::Live, Some(e)) => {
            let mut s = match &e.record {
                Some(name) => format!("{}·{}", e.word.text(), text::clean(name)),
                None => e.word.text(),
            };
            if let Some(source) = &e.source {
                s.push_str(&format!("·{}", text::clean(source)));
            }
            s
        }
        (Life::Live, None) => "live".into(),
    }
}

/// Classify one terminal: lifecycle, effective record, agent inclusion
/// and the urgency used for space aggregation and agent ordering.
pub fn status_of(term: &Terminal) -> TermStatus {
    let life = classify(term);
    let effective = effective(term);
    let agent = term.reported || term.status.is_some() || !term.records.is_empty();
    let urgency = match (&life, effective.as_ref().map(|e| &e.word)) {
        (Life::Live, Some(word)) => word.rank(),
        (Life::Live, None) => 5,
        (Life::Lost, _) => 50,
        (Life::Pending, _) | (Life::Other(_), _) => 5,
        (Life::Finished, _) => 0,
    };
    TermStatus {
        badge: badge(term, &life, effective.as_ref()),
        life,
        effective,
        agent,
        urgency,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use crate::sidebar::fixture::term as bare;

    fn term(state: &str, pid: Option<u32>, reason: Option<&str>) -> Terminal {
        let mut t = bare("t1");
        t.state = state.into();
        t.pid = pid;
        t.reason = reason.map(str::to_string);
        t
    }

    #[test]
    fn lifecycle_matches_core_reasons() {
        assert_eq!(
            status_of(&term("finished", None, None)).life,
            Life::Finished
        );
        assert_eq!(
            status_of(&term("live", None, Some("holder_lost"))).life,
            Life::Lost
        );
        assert_eq!(
            status_of(&term("live", None, Some("spawn_pending"))).life,
            Life::Pending
        );
        assert_eq!(status_of(&term("live", None, None)).life, Life::Pending);
        assert_eq!(status_of(&term("live", Some(7), None)).life, Life::Live);
    }

    #[test]
    fn named_record_can_win_and_stays_named() {
        let mut t = term("live", Some(7), None);
        t.status = Some(Status {
            state: "working".into(),
            ..Status::default()
        });
        t.records.insert(
            "build".into(),
            Status {
                state: "blocked".into(),
                ..Status::default()
            },
        );
        let s = status_of(&t);
        assert_eq!(s.badge, "blocked·build");
        assert_eq!(s.urgency, 70);
        assert!(s.agent);
    }

    #[test]
    fn unknown_words_are_kept_not_relabeled() {
        let mut t = term("live", Some(7), None);
        t.status = Some(Status {
            state: "thinking-hard".into(),
            ..Status::default()
        });
        let s = status_of(&t);
        assert_eq!(s.badge, "thinking-hard");
        assert_eq!(s.urgency, 30);
    }

    #[test]
    fn source_extension_is_carried_verbatim() {
        let mut t = term("live", Some(7), None);
        t.status = Some(Status {
            state: "blocked".into(),
            extra: serde_json::json!({"source": "screen"})
                .as_object()
                .unwrap()
                .clone(),
            ..Status::default()
        });
        assert_eq!(status_of(&t).badge, "blocked·screen");
    }
}
