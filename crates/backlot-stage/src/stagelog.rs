//! The stage's stderr timeline. Every `stage:` line goes through one
//! emitter that collapses a run of consecutive identical lines into
//! `<line> ×N` when the run breaks — a steady loop reads as events,
//! and a repeated write or probe failure stays one line plus a count.
//! Semantic lines (starts, stops, refusals) are distinct, so they
//! always print.

use std::cell::RefCell;

/// Consecutive-duplicate collapse for the stage's log lines.
/// `feed` takes one logical line and answers the text to print now:
/// the new line, preceded by the run's `×N` total when one breaks.
#[derive(Default)]
pub(crate) struct LogLines {
    last: Option<String>,
    extra: u32,
}

impl LogLines {
    /// One timeline line in, what to print out.
    pub(crate) fn feed(&mut self, text: &str) -> Vec<String> {
        if self.last.as_deref() == Some(text) {
            self.extra += 1;
            return Vec::new();
        }
        let mut out = Vec::new();
        if self.extra > 0 {
            out.push(format!(
                "{} ×{}",
                self.last.as_deref().unwrap_or_default(),
                self.extra + 1
            ));
            self.extra = 0;
        }
        out.push(text.to_string());
        self.last = Some(text.to_string());
        out
    }
}

thread_local! {
    /// The single-threaded stage's shared emitter.
    static LINES: RefCell<LogLines> = RefCell::new(LogLines::default());
}

/// Print one `stage:` line through the shared collapse.
pub(crate) fn line(text: &str) {
    LINES.with(|l| {
        for out in l.borrow_mut().feed(text) {
            eprintln!("stage: {out}");
        }
    });
}

/// `stage_log!("… {arg}")` — `eprintln!("stage: …")` through the
/// collapsing timeline.
macro_rules! stage_log {
    ($($t:tt)*) => {
        crate::stagelog::line(&format!($($t)*))
    };
}
pub(crate) use stage_log;
