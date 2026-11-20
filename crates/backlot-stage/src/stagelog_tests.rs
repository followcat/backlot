//! The stage's stderr timeline: a run of identical lines collapses
//! to `<line> ×N` when the run breaks, so a steady loop reads as
//! events and semantic lines stay prominent.

use crate::stagelog::LogLines;

#[test]
fn identical_lines_collapse_to_a_count() {
    let mut l = LogLines::default();
    assert_eq!(l.feed("summary written"), vec!["summary written"]);
    for _ in 0..41 {
        assert!(l.feed("summary written").is_empty());
    }
    assert_eq!(
        l.feed("acme-shop: started producer"),
        vec!["summary written ×42", "acme-shop: started producer"],
        "the run reports its total when it breaks"
    );
}

#[test]
fn alternating_lines_never_collapse() {
    let mut l = LogLines::default();
    assert_eq!(l.feed("a"), vec!["a"]);
    assert_eq!(l.feed("b"), vec!["b"]);
    assert_eq!(l.feed("a"), vec!["a"], "a repeat must be consecutive");
}

#[test]
fn a_single_line_never_gets_a_count() {
    let mut l = LogLines::default();
    assert_eq!(l.feed("summary written"), vec!["summary written"]);
    assert_eq!(
        l.feed("acme-shop: stopping director"),
        vec!["acme-shop: stopping director"],
        "one occurrence needs no ×N"
    );
}
