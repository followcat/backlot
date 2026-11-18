// Derived from herdr src/client/shell/overlays.rs (Apache-2.0): the
// status rows and fenced output box of the update modal.
//! The installer's status bodies inside the update modal: the live
//! `updating` checklist, the verified `done` body and the `failed`
//! body. Every row shows real installer output; there is no progress
//! bar, because the installer prints nothing until it finishes.

use ratatui::style::{Modifier, Style};
use ratatui::text::Line;
use unicode_width::UnicodeWidthStr;

use super::palette::Palette;
use super::release_notes::{
    accent_bold, chip_style, code_rows, line, overlay0, overlay1, text, warn_bold,
};

/// The spinner frames for the running checklist row.
pub(crate) const SPINNER: [&str; 4] = ["◐", "◓", "◑", "◒"];
/// The `failed` body shows at most this many tail rows.
const FAILED_TAIL: usize = 4;
/// The `done` body shows at most this many tail rows.
const DONE_TAIL: usize = 2;
/// Below this body width the status wording is the 60-column variant.
const WIDE: usize = 70;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Step {
    /// The installer is running (`curl … | sh`).
    Running,
    /// The installer exited 0; `<installed> --version` is running.
    Verifying,
    /// Installed and verified.
    Done,
    /// Failed with `reason`.
    Failed,
}

/// The installer state the status bodies draw.
pub(crate) struct Status<'a> {
    pub step: Step,
    pub elapsed_secs: u64,
    pub spinner: usize,
    /// The echoed `$ curl …` pipeline, one row per line.
    pub echo: &'a [String],
    /// The last installer output lines, oldest first.
    pub tail: &'a [String],
    /// `backlot <latest>` once verified.
    pub verified: Option<&'a str>,
    pub reason: Option<&'a str>,
    pub installed: &'a str,
    /// The manual command shown on `failed` (`backlot --upgrade`).
    pub manual: &'a str,
}

/// `M:SS` for the elapsed column.
pub(crate) fn clock(secs: u64) -> String {
    format!("{}:{:02}", secs / 60, secs % 60)
}

/// `left` then `right` pushed to the row's end.
fn split_row(
    mut left: Vec<(String, Style)>,
    right: &str,
    right_style: Style,
    width: usize,
) -> Line<'static> {
    let used: usize = left
        .iter()
        .map(|(t, _)| UnicodeWidthStr::width(t.as_str()))
        .sum();
    let pad = width
        .saturating_sub(used + UnicodeWidthStr::width(right) + 1)
        .max(1);
    left.push((" ".repeat(pad), Style::default()));
    left.push((right.to_owned(), right_style));
    line(&left)
}

/// The fenced output box: the echo rows (if any), a blank row, then
/// the tail — or the waiting note before the first output line.
fn output_box(echo: &[String], tail: &[String], width: usize, p: &Palette) -> Vec<Line<'static>> {
    let mut rows = Vec::new();
    let body = Style::default().fg(p.text);
    let muted = Style::default().fg(p.subtext0);
    for (index, command) in echo.iter().enumerate() {
        let style = if index == 0 { body } else { muted };
        rows.extend(code_rows(command, style, width, p));
    }
    if !echo.is_empty() {
        rows.extend(code_rows("", body, width, p));
    }
    if tail.is_empty() {
        rows.extend(code_rows("waiting for installer output…", muted, width, p));
    }
    for output in tail {
        let style = if output.starts_with("curl:") {
            Style::default().fg(p.red)
        } else {
            body
        };
        rows.extend(code_rows(output, style, width, p));
    }
    rows
}

fn last(tail: &[String], count: usize) -> &[String] {
    &tail[tail.len().saturating_sub(count)..]
}

/// `updating`: the two-step checklist, the echoed pipeline, the live
/// tail, and the cancel note.
pub(crate) fn updating_lines(s: &Status<'_>, width: usize, p: &Palette) -> Vec<Line<'static>> {
    let elapsed = clock(s.elapsed_secs);
    let bold = text(p).add_modifier(Modifier::BOLD);
    let spinner = SPINNER[s.spinner % SPINNER.len()];
    let mut out = Vec::new();
    if s.step == Step::Verifying {
        out.push(split_row(
            vec![
                (" ✓ ".into(), green_bold(p)),
                ("official installer finished".into(), subtext0(p)),
            ],
            &elapsed,
            overlay0(p),
            width,
        ));
        out.push(line(&[
            (" ".into(), text(p)),
            (spinner.into(), warn_bold(p)),
            (" checking new binary (backlot --version)…".into(), bold),
        ]));
    } else {
        out.push(split_row(
            vec![
                (" ".into(), text(p)),
                (spinner.into(), warn_bold(p)),
                (" running official installer…".into(), bold),
            ],
            &elapsed,
            overlay1(p),
            width,
        ));
        out.push(line(&[(
            " · check new binary (backlot --version)".into(),
            overlay0(p),
        )]));
    }
    out.push(Line::raw(""));
    out.push(line(&[(" OUTPUT".into(), accent_bold(p))]));
    out.extend(output_box(s.echo, s.tail, width, p));
    out.push(Line::raw(""));
    out.push(line(&[(
        format!(" cancelling leaves v{} in place", s.installed),
        overlay1(p),
    )]));
    out
}

/// `done`: the verified checklist, the installer's last lines, and the
/// plain restart explanation.
pub(crate) fn done_lines(s: &Status<'_>, width: usize, p: &Palette) -> Vec<Line<'static>> {
    let elapsed = clock(s.elapsed_secs);
    let mut out = vec![
        split_row(
            vec![
                (" ✓ ".into(), green_bold(p)),
                ("official installer finished".into(), subtext0(p)),
            ],
            &elapsed,
            overlay0(p),
            width,
        ),
        line(&[
            (" ✓ ".into(), green_bold(p)),
            (
                "verified new binary: ".into(),
                text(p).add_modifier(Modifier::BOLD),
            ),
            (s.verified.unwrap_or_default().into(), chip_style(p)),
        ]),
        Line::raw(""),
        line(&[(" OUTPUT".into(), accent_bold(p))]),
    ];
    out.extend(output_box(&[], last(s.tail, DONE_TAIL), width, p));
    out.push(Line::raw(""));
    let (first, second) = if width >= WIDE {
        (
            " Restarting re-execs backlot in place and reconnects to this session.",
            " Your hc core, spaces, and running programs keep running.",
        )
    } else {
        (
            " Restarting re-execs backlot and reconnects to this",
            " session. Programs in hc keep running.",
        )
    };
    out.push(line(&[(first.into(), text(p))]));
    out.push(line(&[(second.into(), subtext0(p))]));
    out
}

/// `failed`: the reason, the last output lines, and the manual fallback.
pub(crate) fn failed_lines(s: &Status<'_>, width: usize, p: &Palette) -> Vec<Line<'static>> {
    let elapsed = clock(s.elapsed_secs);
    let red_bold = Style::default().fg(p.red).add_modifier(Modifier::BOLD);
    let mut out = vec![
        split_row(
            vec![
                (" × ".into(), red_bold),
                (s.reason.unwrap_or("update failed").into(), red_bold),
            ],
            &elapsed,
            overlay0(p),
            width,
        ),
        Line::raw(""),
        line(&[(" OUTPUT".into(), accent_bold(p))]),
    ];
    out.extend(output_box(&[], last(s.tail, FAILED_TAIL), width, p));
    out.push(Line::raw(""));
    out.push(line(&[(
        " Upgrade manually from a shell:".into(),
        subtext0(p),
    )]));
    out.push(line(&[
        ("   ".into(), text(p)),
        (s.manual.into(), chip_style(p)),
    ]));
    out.push(Line::raw(""));
    if width >= WIDE {
        out.push(line(&[
            (" or: ".into(), overlay0(p)),
            (
                "curl -fsSL https://github.com/followcat/backlot/releases/latest/download/install.sh | sh".into(),
                overlay1(p),
            ),
        ]));
    } else {
        out.push(line(&[(
            " or run install.sh (see docs/install.md)".into(),
            overlay0(p),
        )]));
    }
    out
}

fn green_bold(p: &Palette) -> Style {
    Style::default().fg(p.green).add_modifier(Modifier::BOLD)
}

fn subtext0(p: &Palette) -> Style {
    Style::default().fg(p.subtext0)
}
