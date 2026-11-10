//! The check runner's contract, driven through injected children:
//! a timeout escalates TERM then KILL five seconds later, a signal
//! exit records no code, the capture keeps its last MiB, and
//! `env_for` shows exactly what crosses into the stripped
//! environment.

use std::collections::{BTreeMap, VecDeque};
use std::sync::{Arc, Mutex};

use crate::checkrun::{self, CheckChild, CheckSpawn, PollOut};
use crate::checks;
use crate::checkwork::Running;

/// A scripted child: `exits` queues poll answers — `None` keeps it
/// alive, `Some(code)` finishes it; `terms`/`kills` count
/// escalations; `until_term` delays the queued exit until TERM
/// lands.
struct Fake {
    exits: VecDeque<Option<i32>>,
    terms: usize,
    kills: usize,
    until_term: bool,
}

impl Fake {
    fn alive() -> Fake {
        Fake {
            exits: VecDeque::new(),
            terms: 0,
            kills: 0,
            until_term: false,
        }
    }

    fn exits(codes: &[Option<i32>]) -> Fake {
        Fake {
            exits: codes.iter().copied().collect(),
            ..Fake::alive()
        }
    }
}

impl CheckChild for Fake {
    fn poll(&mut self) -> Option<PollOut> {
        if self.until_term && self.terms == 0 {
            return None;
        }
        self.exits.pop_front().map(|code| PollOut {
            code,
            output: b"the log".to_vec(),
        })
    }

    fn term(&mut self) {
        self.terms += 1;
    }

    fn kill(&mut self) {
        self.kills += 1;
    }
}

/// The deadline sends TERM once; five seconds later, still alive,
/// KILL follows once — then the stage only keeps polling.
#[test]
fn a_timeout_terms_then_kills() {
    let fake = Arc::new(Mutex::new(Fake::alive()));
    struct Spy(Arc<Mutex<Fake>>);
    impl CheckChild for Spy {
        fn poll(&mut self) -> Option<PollOut> {
            self.0.lock().unwrap().poll()
        }
        fn term(&mut self) {
            self.0.lock().unwrap().term();
        }
        fn kill(&mut self) {
            self.0.lock().unwrap().kill();
        }
    }
    let mut run = Running::for_test(Box::new(Spy(Arc::clone(&fake))), 1_000, Default::default());
    assert!(run.poll(999).is_none());
    assert_eq!(fake.lock().unwrap().terms, 0);
    assert!(run.poll(1_000).is_none());
    assert_eq!(fake.lock().unwrap().terms, 1);
    // Not re-termed while the five seconds run.
    assert!(run.poll(1_500).is_none());
    assert_eq!(fake.lock().unwrap().terms, 1);
    // KILL lands five seconds after TERM, once.
    assert!(run.poll(5_999).is_none());
    assert_eq!(fake.lock().unwrap().kills, 0);
    assert!(run.poll(6_000).is_none());
    assert_eq!(fake.lock().unwrap().kills, 1);
    assert!(run.poll(7_000).is_none());
    assert_eq!(fake.lock().unwrap().kills, 1);
}

/// A signal-ended run records no exit code and is not a timeout.
#[test]
fn a_signal_exit_records_no_code() {
    let mut run = Running::for_test(Box::new(Fake::exits(&[None])), 60_000, Default::default());
    let out = run.poll(500).expect("finished");
    assert_eq!(out.exit_code, None);
    assert!(!out.timed_out);
    assert_eq!(out.output, b"the log");
    assert_eq!(out.duration_ms, 500);
}

/// A clean exit reports its code and is never a timeout.
#[test]
fn a_clean_exit_reports_its_code() {
    let mut run = Running::for_test(
        Box::new(Fake::exits(&[Some(3)])),
        60_000,
        Default::default(),
    );
    let out = run.poll(500).expect("finished");
    assert_eq!(out.exit_code, Some(3));
    assert!(!out.timed_out);
}

/// An exit at the deadline is a natural exit — no escalation, not
/// timed out — but once TERM went out the run is timed out however
/// the process then dies, even on code 0.
#[test]
fn the_timeout_owns_the_exit_it_forced() {
    let mut run = Running::for_test(Box::new(Fake::exits(&[Some(0)])), 1_000, Default::default());
    let out = run.poll(1_000).expect("finished");
    assert_eq!(out.exit_code, Some(0));
    assert!(!out.timed_out, "the exit beat the deadline");

    let mut run = Running::for_test(
        Box::new(Fake {
            until_term: true,
            ..Fake::exits(&[Some(0)])
        }),
        1_000,
        Default::default(),
    );
    assert!(run.poll(1_000).is_none(), "TERM goes out first");
    let out = run.poll(1_001).expect("finished");
    assert_eq!(out.exit_code, Some(0));
    assert!(out.timed_out, "code 0 after TERM is still timed out");
}

/// The capture keeps only its last MiB — a flooded pipe costs the
/// beginning, never the end.
#[test]
fn the_log_keeps_its_last_mib() {
    let mut buf = checkrun::CappedBuf(Vec::new());
    buf.push(&vec![b'a'; checkrun::LOG_CAP]);
    buf.push(&[b'b'; 100]);
    assert_eq!(buf.0.len(), checkrun::LOG_CAP);
    assert_eq!(
        &buf.0[..checkrun::LOG_CAP - 100],
        &[b'a'; checkrun::LOG_CAP - 100][..]
    );
    assert_eq!(&buf.0[checkrun::LOG_CAP - 100..], &[b'b'; 100][..]);
    // A single oversized push still keeps the end.
    let mut buf = checkrun::CappedBuf(Vec::new());
    let big: Vec<u8> = (0..checkrun::LOG_CAP * 2)
        .map(|i| (i % 251) as u8)
        .collect();
    buf.push(&big);
    assert_eq!(buf.0.len(), checkrun::LOG_CAP);
    assert_eq!(buf.0, big[checkrun::LOG_CAP..]);
}

/// `env_for`: only the passthrough names plus declared `check_env`
/// names, valued from the stage's environment. `BACKLOT_*`, `HC_*`
/// and proxies never cross, and a name the stage lacks is absent.
#[test]
fn the_environment_is_the_short_list() {
    let stage: BTreeMap<String, String> = [
        ("PATH", "/usr/bin"),
        ("HOME", "/home/user"),
        ("HTTPS_PROXY", "http://proxy.invalid:1"),
        ("BACKLOT_ROLE", "x"),
        ("HC_SESSION", "s"),
        ("CARGO_HOME", "/home/user/.cargo"),
        ("RANDOM_THING", "no"),
    ]
    .iter()
    .map(|(k, v)| (k.to_string(), v.to_string()))
    .collect();
    let env = checks::env_for(&stage, &["CARGO_HOME".into(), "UNSET_VAR".into()]);
    assert_eq!(
        env,
        vec![
            ("PATH".to_string(), "/usr/bin".to_string()),
            ("HOME".to_string(), "/home/user".to_string()),
            ("CARGO_HOME".to_string(), "/home/user/.cargo".to_string()),
        ]
    );
}

/// The real spawn: `sh -c` under the given env, both pipes into the
/// capture, exit code out of `poll`.
#[test]
fn real_spawn_runs_sh() {
    let mut child = checkrun::real_spawn(&CheckSpawn {
        run: "echo out; echo err >&2; exit 3".into(),
        dir: std::env::temp_dir(),
        env: vec![("PATH".into(), "/usr/bin:/bin".into())],
    })
    .expect("spawn");
    let mut out = None;
    for _ in 0..200 {
        if let Some(o) = child.poll() {
            out = Some(o);
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
    let out = out.expect("the run finished");
    assert_eq!(out.code, Some(3));
    let text = String::from_utf8_lossy(&out.output);
    assert!(text.contains("out"), "{text}");
    assert!(text.contains("err"), "{text}");
}
