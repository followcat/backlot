//! The `deliver --commits` checks on a scripted git: resolve both
//! ends, refuse an empty range, refuse a tip off the task branch —
//! each refusal names which check failed.

use std::path::PathBuf;

use crate::range;
use crate::workareas::GitOut;

/// A scripted git: `rev-parse` answers `sha-<spec>` (a `bad*` spec
/// fails like a real unresolvable rev), `merge-base --is-ancestor`
/// answers from `ancestors` (default 1, "not an ancestor").
fn fake_git(ancestors: &[(&str, &str, i32)]) -> impl FnMut(&[&str], &str) -> GitOut + use<> {
    let pairs: Vec<(String, String, i32)> = ancestors
        .iter()
        .map(|(a, b, c)| (a.to_string(), b.to_string(), *c))
        .collect();
    move |argv, _cwd| match argv {
        ["rev-parse", "--verify", "--end-of-options", spec] => {
            if spec.starts_with("bad") {
                GitOut {
                    code: 128,
                    stdout: String::new(),
                    stderr: "fatal: Needed a single revision\n".into(),
                }
            } else {
                GitOut {
                    code: 0,
                    stdout: format!("sha-{}\n", spec.trim_end_matches("^{commit}")),
                    stderr: String::new(),
                }
            }
        }
        ["merge-base", "--is-ancestor", a, b] => GitOut {
            code: pairs
                .iter()
                .find(|(x, y, _)| x == a && y == b)
                .map(|p| p.2)
                .unwrap_or(1),
            stdout: String::new(),
            stderr: String::new(),
        },
        other => panic!("unexpected git call {other:?}"),
    }
}

const BRANCH: &str = "backlot/acme-shop/t1";

/// Both ends resolve, the range is non-empty and its tip sits on the
/// task's branch: the recorded form is the resolved `sha..sha`.
#[test]
fn resolved_form_is_sha_to_sha() {
    let work = PathBuf::from("/w");
    let mut git = fake_git(&[("sha-HEAD", "sha-main", 1), ("sha-HEAD", BRANCH, 0)]);
    let out = range::resolve(&mut git, &work, BRANCH, "main..HEAD").unwrap();
    assert_eq!(out, "sha-main..sha-HEAD");
}

/// `A..B` where B is already reachable from A delivers nothing.
#[test]
fn an_empty_range_is_refused() {
    let work = PathBuf::from("/w");
    for (a, b) in [("a", "b"), ("x", "x")] {
        let (sa, sb) = (format!("sha-{a}"), format!("sha-{b}"));
        let mut git = fake_git(&[(sb.as_str(), sa.as_str(), 0)]);
        let msg = range::resolve(&mut git, &work, BRANCH, &format!("{a}..{b}"))
            .unwrap_err()
            .message()
            .to_string();
        assert!(msg.contains("empty range"), "{a}..{b}: {msg}");
    }
}

/// `B` must be an ancestor-or-self of `backlot/<studio>/<task>` —
/// work on a side branch is not a delivery.
#[test]
fn a_tip_off_the_task_branch_is_refused() {
    let work = PathBuf::from("/w");
    let mut git = fake_git(&[("sha-b", "sha-a", 1), ("sha-b", BRANCH, 1)]);
    let msg = range::resolve(&mut git, &work, BRANCH, "a..b")
        .unwrap_err()
        .message()
        .to_string();
    assert!(msg.contains("not on branch"), "{msg}");
    assert!(msg.contains(BRANCH), "{msg}");
}

/// An endpoint that does not resolve names itself; a malformed
/// range is a usage error before git runs.
#[test]
fn an_unresolvable_end_is_named() {
    let work = PathBuf::from("/w");
    let mut git = fake_git(&[]);
    let msg = range::resolve(&mut git, &work, BRANCH, "a..bad-x")
        .unwrap_err()
        .message()
        .to_string();
    assert!(msg.contains("cannot resolve 'bad-x'"), "{msg}");
    for bad in ["a", "..b", "a..", ".."] {
        let mut git = fake_git(&[]);
        let err = range::resolve(&mut git, &work, BRANCH, bad).unwrap_err();
        assert!(err.message().contains("want A..B"), "{bad}: {err:?}");
    }
}
