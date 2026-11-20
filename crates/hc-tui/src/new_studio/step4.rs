//! Step 4 — the dry-run verdict, the summary card and the
//! start-stage checkbox. `Enter` runs `studio new` on the target
//! machine, then `stage start` when the box is checked, and the
//! new producer's terminal takes focus.

use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::{Modifier, Style};

use super::{NewStudio, NsHit, NsHits, Probe, selected_row};
use crate::chrome::palette::Palette;
use crate::chrome::render::{put_segment, put_text};

/// A `label: value` line of the summary card.
fn card_line(b: &mut Buffer, body: Rect, y: u16, p: &Palette, label: &str, value: &str) {
    let x = put_segment(
        b,
        body.x + 1,
        y,
        body.right(),
        &format!("{label:<12}"),
        Style::default()
            .fg(p.overlay0)
            .bg(p.panel_bg)
            .add_modifier(Modifier::BOLD),
    );
    put_segment(
        b,
        x,
        y,
        body.right(),
        &crate::chrome::text::truncate_end(value, body.right().saturating_sub(x) as usize),
        Style::default().fg(p.text).bg(p.panel_bg),
    );
}

pub(crate) fn draw(b: &mut Buffer, body: Rect, d: &NewStudio, p: &Palette, hits: &mut NsHits) {
    b.set_style(body, Style::default().bg(p.panel_bg));
    let machine = d.machine_label();
    let mut y = body.y;

    // The dry-run verdict line, with the command on the right.
    let (mark, text, style) = match &d.dry {
        Probe::Pending => (
            "…",
            format!("dry-run on {machine}"),
            Style::default().fg(p.yellow).bg(p.panel_bg),
        ),
        Probe::Done(_) => (
            "✓",
            format!("dry-run passed on {machine}"),
            Style::default().fg(p.green).bg(p.panel_bg),
        ),
        Probe::Failed(_) => (
            "×",
            format!("dry-run failed on {machine}"),
            Style::default().fg(p.red).bg(p.panel_bg),
        ),
    };
    let x = put_segment(b, body.x + 1, y, body.right(), mark, style);
    put_segment(
        b,
        x + 1,
        y,
        body.right(),
        &crate::chrome::text::truncate_end(&text, body.right().saturating_sub(x + 1) as usize),
        style,
    );
    // The command line carries the flag the argv will carry — a
    // plain directory's `--init`/`--no-git` — so the check the user
    // reads is the check the stage runs.
    let flag = match d.repo().map(|r| r.shape()) {
        Some(super::PathShape::Plain) if d.no_git => " --no-git",
        Some(super::PathShape::Plain) => " --init",
        _ => "",
    };
    let cmd = format!("backlot studio new{flag} --dry-run");
    if display_fits(body.width, &text, &cmd) {
        put_text(
            b,
            body.right().saturating_sub(cmd.len() as u16),
            y,
            cmd.len() as u16,
            &cmd,
            Style::default().fg(p.overlay0).bg(p.panel_bg),
        );
    }
    y += 1;

    // A failed dry run's own words — verbatim, as many lines as fit.
    if let Probe::Failed(error) = &d.dry {
        for line in error.lines().take(4) {
            if y >= body.bottom().saturating_sub(6) {
                break;
            }
            put_text(
                b,
                body.x + 1,
                y,
                body.width.saturating_sub(1),
                &crate::chrome::text::truncate_end(line, body.width as usize - 2),
                Style::default().fg(p.red).bg(p.panel_bg),
            );
            y += 1;
        }
    }
    y += 1;

    // The summary card.
    let repo = d.repo();
    let version = d
        .machine()
        .and_then(|m| match &m.backlot {
            Probe::Done(v) => Some(v.clone()),
            _ => None,
        })
        .unwrap_or_else(|| "?".into());
    card_line(
        b,
        body,
        y,
        p,
        "Machine",
        &format!("{machine} (backlot {version})"),
    );
    y += 1;
    if let Some(repo) = repo {
        let mut value = repo.path.clone();
        if let Some(branch) = &repo.branch {
            value.push_str(&format!(" ({branch})"));
        }
        card_line(b, body, y, p, "Repository", &value);
        y += 1;
    }
    card_line(
        b,
        body,
        y,
        p,
        "Studio",
        &format!("{} — \"{}\"", d.id.as_str(), d.title.as_str()),
    );
    y += 1;
    let runners = match &d.runners {
        Probe::Done(r) => r.as_slice(),
        _ => &[],
    };
    let cast: Vec<String> = d
        .cast
        .rows
        .iter()
        .map(|row| {
            let runner = runners.get(row.runner);
            let model = runner
                .and_then(|r| r.models.get(row.model))
                .map(String::as_str)
                .unwrap_or("-");
            format!("{} {}", row.key, model)
        })
        .collect();
    card_line(
        b,
        body,
        y,
        p,
        "Cast",
        &crate::chrome::text::truncate_end(&cast.join(" · "), body.width as usize - 14),
    );
    y += 1;
    card_line(
        b,
        body,
        y,
        p,
        "Launch",
        &format!(
            "creates studio {} on {machine} & opens producer",
            d.id.as_str()
        ),
    );
    y += 1;
    // D4/D19: the launch plan restates the bound path's shape and
    // its implications — the same text the picker's labels promise.
    if let Some(repo) = repo {
        let plan = match repo.shape() {
            super::PathShape::Workspace => {
                format!(
                    "workspace over {} repos: {}",
                    repo.members.len(),
                    repo.members.join(", ")
                )
            }
            super::PathShape::Plain if d.no_git => {
                "no git (reduced isolation) — shared path, no worktrees".to_string()
            }
            super::PathShape::Plain => {
                let on = match d.machine_name() {
                    Some(name) => format!(" on {name}"),
                    None => String::new(),
                };
                format!(
                    "will run: git init && git commit --allow-empty -m \"chore: initial commit\" in {}{on}",
                    repo.path
                )
            }
            super::PathShape::Repo if repo.kind == super::RepoKind::GitEmpty => {
                format!("will create initial commit in {}", repo.path)
            }
            super::PathShape::Repo => String::new(),
        };
        if !plan.is_empty() {
            card_line(b, body, y, p, "Plan", &plan);
            y += 1;
        }
    }
    y += 1;

    // A plain-directory pick offers the explicit no-git toggle —
    // the degraded mode lives only here on the launch check (D4).
    if repo.is_some_and(|r| r.shape() == super::PathShape::Plain) && y < body.bottom() {
        let row = Rect::new(body.x, y, body.width, 1);
        let check = if d.no_git { "[x]" } else { "[ ]" };
        put_text(
            b,
            body.x + 1,
            y,
            body.width.saturating_sub(1),
            &format!(" {check} no git (reduced isolation)"),
            Style::default().fg(p.text).bg(p.panel_bg),
        );
        hits.push(row, NsHit::NoGitToggle);
        y += 1;
    }
    y += 1;

    // The start-stage checkbox row + the Create button.
    if y >= body.bottom() {
        return;
    }
    let row = Rect::new(body.x, y, body.width, 1);
    selected_row(b, row, p);
    put_text(
        b,
        body.x,
        y,
        1,
        "▸",
        Style::default().fg(p.accent).bg(p.surface0),
    );
    let check = if d.start_stage { "[x]" } else { "[ ]" };
    let toggle = format!(" {check} Ensure stage is running on {machine} and focus producer");
    put_text(
        b,
        body.x + 1,
        y,
        body.width.saturating_sub(1),
        &crate::chrome::text::truncate_end(&toggle, body.width as usize - 10),
        Style::default().fg(p.text).bg(p.surface0),
    );
    hits.push(row, NsHit::StartToggle);
    let button = " Create ⏎ ";
    let bw = button.len() as u16;
    if body.width > bw + 4 {
        let rect = Rect::new(body.right().saturating_sub(bw + 1), y, bw, 1);
        put_text(
            b,
            rect.x,
            y,
            bw,
            button,
            Style::default()
                .fg(crate::chrome::dialogs::contrast(p))
                .bg(p.accent)
                .add_modifier(Modifier::BOLD),
        );
        hits.push(rect, NsHit::Create);
    }
    y += 1;

    // Creating's pending line, or the finished call's error.
    if d.creating && y < body.bottom() {
        put_text(
            b,
            body.x + 1,
            y,
            body.width.saturating_sub(1),
            &crate::chrome::text::truncate_end(
                &format!("creating studio on {machine}…"),
                body.width.saturating_sub(1) as usize,
            ),
            Style::default().fg(p.yellow).bg(p.panel_bg),
        );
    } else if let Some(error) = &d.create_error {
        for line in error.lines().take(3) {
            if y >= body.bottom() {
                break;
            }
            put_text(
                b,
                body.x + 1,
                y,
                body.width.saturating_sub(1),
                &crate::chrome::text::truncate_end(line, body.width as usize - 2),
                Style::default().fg(p.red).bg(p.panel_bg),
            );
            y += 1;
        }
    }
}

pub(super) fn display_fits(width: u16, left: &str, right: &str) -> bool {
    (left.len() + right.len() + 4) < width as usize
}
