//! Per-terminal status classification. The only authority is what core
//! reports in the record — `status`/`records` (OSC 7501), `state`,
//! `pid`, `reason` — never screen text, argv or foreground names.
//! Semantics are written down in docs/sidebar.md.
//!
//! The row/badge presentation that used to live here moved with the
//! chrome port: `project` maps `classify`/`effective` straight onto the
//! reference `AgentStatus`, so the custom `word·name`/`·source` badge
//! assembly no longer exists.

use hc_proto::terminal::{Status, Terminal};

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

pub(crate) fn classify(term: &Terminal) -> Life {
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
pub(crate) fn effective(term: &Terminal) -> Option<Effective> {
    let mut best: Option<(u8, Effective)> = None;
    let mut offer = |record: Option<String>, status: &Status| {
        let word = word_of(status);
        let rank = word.rank();
        let wins = best.as_ref().is_none_or(|(r, _)| rank > *r);
        if wins {
            best = Some((rank, Effective { word, record }));
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
        assert_eq!(classify(&term("finished", None, None)), Life::Finished);
        assert_eq!(
            classify(&term("live", None, Some("holder_lost"))),
            Life::Lost
        );
        assert_eq!(
            classify(&term("live", None, Some("spawn_pending"))),
            Life::Pending
        );
        assert_eq!(classify(&term("live", None, None)), Life::Pending);
        assert_eq!(classify(&term("live", Some(7), None)), Life::Live);
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
        let e = effective(&t).unwrap();
        assert_eq!(e.word, Word::Blocked { kind: None });
        assert_eq!(e.record.as_deref(), Some("build"));
    }

    #[test]
    fn unknown_words_are_kept_not_relabeled() {
        let mut t = term("live", Some(7), None);
        t.status = Some(Status {
            state: "thinking-hard".into(),
            ..Status::default()
        });
        let e = effective(&t).unwrap();
        assert_eq!(e.word, Word::Other("thinking-hard".into()));
    }
}
