//! The studio cast (U1): in studio mode the sidebar's agents section
//! becomes the current studio's people — two rows each, the focused
//! person marked, closed people last — and an `others` row that
//! expands into herdr's own agent rows. Everything the renderer needs
//! is a borrow of the summaries plus the terminal inventory's titles.

mod draw;
pub(crate) mod rows;
mod simple;

#[cfg(test)]
#[path = "tests.rs"]
mod tests;

use std::collections::BTreeMap;

use ratatui::layout::Rect;

use crate::chrome::ClientShellConfig;
use crate::chrome::agents::{AgentRow, agent_rows};
use crate::chrome::snapshot::ClientShellSnapshot;
use crate::chrome::title_text::stripped_terminal_title;
use crate::panel::data::{MachineSummary, StudioLabel, StudioRef, spans_machines, studio_label};
use crate::studio::StudioKey;

pub(crate) use draw::render_cast;

/// What the cast draws with, built per frame: the current studio, its
/// position, the focused terminal and the stripped terminal titles.
pub(crate) struct CastView<'a> {
    /// The current studio — `None` while no machine's document lists
    /// one: the cast still draws, people-less, so the `others` group
    /// is where stray terminals land (U2's no-studio state).
    pub(crate) studio: Option<StudioRef<'a>>,
    /// The header's `id · machine` label pair — the machine part only
    /// exists while studios span machines.
    pub(crate) label: Option<StudioLabel>,
    /// Its position in the aggregate's order (0-based) and the studio
    /// count — the header's `<n>/<total>`.
    pub(crate) index: usize,
    pub(crate) total: usize,
    /// The focused terminal — its owner gets the active rows.
    pub(crate) focused_terminal: Option<&'a str>,
    /// `terminal → stripped title` for a person's second row.
    pub(crate) titles: BTreeMap<String, Option<String>>,
    /// The studio's shells (U2) — the `shells` group after the
    /// people, in shell-number order, on the studio's machine.
    pub(crate) shells: Vec<ShellRow>,
    /// Herdr's agents that are no studio person, studio shell or
    /// backlot-owned terminal — the `others` row's count and,
    /// expanded, its rows.
    pub(crate) others: Vec<AgentRow>,
    /// Whether `others` is expanded in place.
    pub(crate) others_expanded: bool,
    /// The working-mark spinner frame (~120 ms ticks).
    pub(crate) spin: usize,
    /// `true` when `StudioFocus::Studio(_)` owns focus rather than any role.
    pub(crate) studio_focused: bool,
    /// C1: no studio is current but more than one exists — the body's
    /// one dim line `select a studio (Enter on it)`.
    pub(crate) hint: bool,
}

impl<'a> CastView<'a> {
    /// Build the frame's cast: the studio `current` points at; with no
    /// current studio the only live one, so a single-studio lot is
    /// never blank (C1). With more than one and none current the body
    /// draws the `select a studio` hint. The cast always draws so the
    /// `others` row exists.
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn build(
        machines: &'a [MachineSummary],
        snapshot: &'a ClientShellSnapshot,
        view: &'a hc_proto::watch::View,
        current: Option<&'a StudioKey>,
        local: Option<&'a str>,
        config: &'a ClientShellConfig,
        others_expanded: bool,
        focused: Option<&'a str>,
        spin: usize,
    ) -> CastView<'a> {
        let studios = crate::panel::data::studios(machines);
        let index = current.and_then(|key| studios.iter().position(|s| &s.key() == key));
        // C1: the cast explains itself. A current studio wins; with no
        // current studio exactly one live studio shows its cast; more
        // than one draws the `select a studio` hint.
        let live: Vec<usize> = studios
            .iter()
            .enumerate()
            .filter(|(_, studio)| studio.studio.state != "finished")
            .map(|(index, _)| index)
            .collect();
        let (index, studio, hint) = match index {
            Some(index) => (index, Some(studios[index]), false),
            None if live.len() == 1 => (live[0], Some(studios[live[0]]), false),
            _ => (0, None, !studios.is_empty()),
        };
        let multi = spans_machines(machines);
        let label = studio.map(|s| studio_label(&s.studio.id, s.machine, local, multi));
        // Every person of every studio is out of `others`, and so is
        // anything backlot owns: a `backlot.role` (a person, the
        // stage, Hollywood), a `backlot.shell` or a `backlot.studio`
        // label keeps a terminal out of the row. Remote people's
        // terminals arrive machine-qualified already.
        let mut seated: std::collections::BTreeSet<&str> = studios
            .iter()
            .flat_map(|s| s.studio.roles.iter())
            .filter_map(|role| role.terminal.as_deref())
            .collect();
        seated.extend(
            view.inventory
                .terminals()
                .values()
                .filter(|t| {
                    t.labels
                        .keys()
                        .any(|k| k == crate::shells::SHELL_LABEL || k.starts_with("backlot."))
                })
                .map(|t| t.address.as_str()),
        );
        let others = agent_rows(snapshot, config, None)
            .into_iter()
            .filter(|row| !seated.contains(row.pane_id.as_str()))
            .collect();
        let titles = studio
            .map(|studio| {
                studio
                    .studio
                    .roles
                    .iter()
                    .filter_map(|role| role.terminal.clone())
                    .map(|terminal| {
                        let title = view
                            .inventory
                            .get(&terminal)
                            .and_then(|term| stripped_terminal_title(&term.title));
                        (terminal, title)
                    })
                    .collect()
            })
            .unwrap_or_default();
        let shells = studio
            .map(|studio| {
                crate::shells::studio_shells(view, studio.machine, &studio.studio.id)
                    .into_iter()
                    .filter_map(|(n, address)| Some((n, view.inventory.get(&address)?)))
                    .map(|(n, term)| ShellRow {
                        n,
                        terminal: term.address.clone(),
                        program: term
                            .argv
                            .first()
                            .and_then(|a| a.rsplit('/').next())
                            .unwrap_or("shell")
                            .to_owned(),
                        title: stripped_terminal_title(&term.title),
                        cwd_tail: term
                            .cwd
                            .trim_end_matches('/')
                            .rsplit('/')
                            .next()
                            .unwrap_or(&term.cwd)
                            .to_owned(),
                        mark: shell_mark(term),
                    })
                    .collect()
            })
            .unwrap_or_default();
        CastView {
            studio,
            label,
            index,
            total: studios.len(),
            focused_terminal: focused,
            titles,
            shells,
            others,
            others_expanded,
            spin,
            studio_focused: false,
            hint,
        }
    }
}

/// One `shells`-group entry's data — everything the two display
/// lines need, resolved at build time.
pub(crate) struct ShellRow {
    /// `backlot.shell`'s number — `shell <n>`'s `<n>`.
    pub(crate) n: u32,
    /// The shell's terminal address.
    pub(crate) terminal: String,
    /// argv[0]'s basename — the right-aligned program.
    pub(crate) program: String,
    /// The stripped terminal title, when the shell set one.
    pub(crate) title: Option<String>,
    /// The working directory's last component.
    pub(crate) cwd_tail: String,
    /// The agent-status mark when the shell runs an agent — `None`
    /// for a plain shell, which shows a dim `$`.
    pub(crate) mark: Option<crate::studio::glyphs::Person>,
}

/// A shell's mark: the terminal's 7501 status word mapped to the
/// shared vocabulary — a shell's prompt is the user's.
fn shell_mark(term: &hc_proto::terminal::Terminal) -> Option<crate::studio::glyphs::Person> {
    use crate::studio::glyphs::Person;
    term.status
        .as_ref()
        .map(|status| match status.state.as_str() {
            "working" => Person::Working,
            "blocked" => Person::UserPrompt,
            "done" | "closed" => Person::Closed,
            _ => Person::Idle,
        })
}

/// The spaces:cast split in studio mode — the spaces section takes
/// only the rows it needs, at most a third of the sidebar height, and
/// the cast gets the rest. Replaces `expanded_sidebar_sections`'s
/// draggable split while the cast is up.
pub(crate) fn sections(area: Rect, spaces_needed: u16) -> (Rect, Rect) {
    let content = Rect::new(area.x, area.y, area.width.saturating_sub(1), area.height);
    if content.is_empty() {
        return (Rect::default(), Rect::default());
    }
    let workspace = spaces_needed
        .min((content.height / 3).max(1))
        .clamp(1, content.height.saturating_sub(1).max(1));
    (
        Rect::new(content.x, content.y, content.width, workspace),
        Rect::new(
            content.x,
            content.y + workspace,
            content.width,
            content.height - workspace,
        ),
    )
}
