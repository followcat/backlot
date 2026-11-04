//! The sidebar: a pure model over the `watch::View` plus a pure
//! ratatui renderer. `Sidebar::update` consumes a snapshot and rebuilds
//! rows/counters; selection tracks stable row identities so reorders
//! and events keep it, and removal falls back deterministically.
//! Semantics are documented in docs/sidebar.md.

mod model;
mod render;
mod rows;
mod status;
mod text;

pub use render::{EXPAND_MIN, Place, draw, place};
pub use rows::{Counters, Row, RowId, Summary, Tone, ViewMode};

/// Width below which the sidebar collapses to its compact line is the
/// renderer's call; `Sidebar::expanded` applies the user's toggle.
pub struct Sidebar {
    mode: ViewMode,
    rows: Vec<Row>,
    counters: Counters,
    summary: Summary,
    /// The selected row's stable identity.
    selected: Option<RowId>,
    /// Where the selection sat last build — the deterministic fallback
    /// when its identity disappears.
    position: usize,
    /// User override for expanded/compact (`None` = width decides).
    force: Option<bool>,
}

impl Default for Sidebar {
    fn default() -> Sidebar {
        Sidebar {
            mode: ViewMode::Spaces,
            rows: Vec::new(),
            counters: Counters::default(),
            summary: Summary::default(),
            selected: None,
            position: 0,
            force: None,
        }
    }
}

impl Sidebar {
    /// An empty sidebar in spaces mode.
    pub fn new() -> Sidebar {
        Sidebar::default()
    }

    /// Rebuild rows and counters from a fresh watcher view. The
    /// selected identity keeps its new position; a vanished identity
    /// falls back to whatever row now holds its old index.
    pub fn update(&mut self, view: &hc_proto::watch::View) {
        let (rows, counters, summary) = model::build(view, self.mode);
        self.rows = rows;
        self.counters = counters;
        self.summary = summary;
        self.resolve();
    }

    /// Resolve `selected` to an index in the current rows.
    fn resolve(&mut self) {
        if self.rows.is_empty() {
            self.selected = None;
            self.position = 0;
            return;
        }
        let ix = self
            .selected
            .as_ref()
            .and_then(|id| self.rows.iter().position(|r| &r.id == id))
            .unwrap_or_else(|| self.position.min(self.rows.len() - 1));
        self.position = ix;
        self.selected = Some(self.rows[ix].id.clone());
    }

    /// Select by index; identity follows.
    fn select_ix(&mut self, ix: usize) {
        if self.rows.is_empty() {
            return;
        }
        self.position = ix.min(self.rows.len() - 1);
        self.selected = Some(self.rows[self.position].id.clone());
    }

    /// One row down, clamped at the bottom.
    pub fn next(&mut self) {
        self.select_ix(self.position.saturating_add(1));
    }

    /// One row up, clamped at the top.
    pub fn prev(&mut self) {
        self.select_ix(self.position.saturating_sub(1));
    }

    /// To the first row.
    pub fn first(&mut self) {
        self.select_ix(0);
    }

    /// To the last row.
    pub fn last(&mut self) {
        self.select_ix(self.rows.len().saturating_sub(1));
    }

    /// Which list is shown.
    pub fn mode(&self) -> ViewMode {
        self.mode
    }

    /// Switch spaces/agents. Rows are rebuilt by the next `update`;
    /// selection resets to the top — a space row has no meaning in the
    /// agents list and vice versa.
    pub fn set_mode(&mut self, mode: ViewMode) {
        if self.mode != mode {
            self.mode = mode;
            self.selected = None;
            self.position = 0;
        }
    }

    /// The other mode (spaces ⇄ agents).
    pub fn toggle_mode(&mut self) {
        self.set_mode(match self.mode {
            ViewMode::Spaces => ViewMode::Agents,
            ViewMode::Agents => ViewMode::Spaces,
        });
    }

    /// Whether the sidebar renders expanded at `width`. Below
    /// [`EXPAND_MIN`] it auto-collapses unless the user forced it.
    pub fn expanded(&self, width: u16) -> bool {
        self.force.unwrap_or(width >= EXPAND_MIN)
    }

    /// Flip expanded/compact for the current width, remembered until
    /// toggled back.
    pub fn toggle(&mut self, width: u16) {
        self.force = Some(!self.expanded(width));
    }

    /// The current rows (all sanitized).
    pub fn rows(&self) -> &[Row] {
        &self.rows
    }

    /// The headline counters.
    pub fn counters(&self) -> Counters {
        self.counters
    }

    /// Machine/offline summary for the compact line.
    pub fn summary(&self) -> &Summary {
        &self.summary
    }

    /// The selected row's identity.
    pub fn selected(&self) -> Option<&RowId> {
        self.selected.as_ref()
    }

    /// The selected row's index in [`rows`](Self::rows).
    pub fn selected_index(&self) -> Option<usize> {
        self.selected.as_ref().map(|_| self.position)
    }

    /// The selected terminal's localized address, when the selection
    /// sits on a terminal row.
    pub fn selected_terminal(&self) -> Option<&str> {
        match &self.selected {
            Some(RowId::Terminal(address)) => Some(address),
            _ => None,
        }
    }
}

/// A minimal live terminal record for tests; tweak fields as needed.
#[cfg(test)]
pub(crate) mod fixture {
    use hc_proto::terminal::Terminal;

    pub(crate) fn term(address: &str) -> Terminal {
        Terminal {
            address: address.into(),
            name: None,
            labels: Default::default(),
            argv: vec!["sh".into()],
            cwd: "/home/user/proj".into(),
            keep: false,
            state: "live".into(),
            pid: Some(100),
            exit_code: None,
            signal: None,
            reason: None,
            created_ms: 0,
            finished_ms: None,
            screen: "exact".into(),
            cols: 80,
            rows: 24,
            identity: String::new(),
            status: None,
            status_ms: None,
            records: Default::default(),
            turn: 0,
            reported: false,
            waiting_for_person: false,
            title: String::new(),
            parent: None,
            notify: false,
            die_with_parent: false,
            rm: false,
            template: None,
            first_message: None,
            submit: None,
            stop: None,
            paste_gap_ms: None,
            foreground: None,
            extra: Default::default(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::fixture::term;
    use super::*;
    use hc_proto::watch::View;

    fn view_with(addresses: &[&str]) -> View {
        let mut view = View::default();
        let terminals = addresses
            .iter()
            .map(|a| {
                let mut t = term(a);
                t.pid = Some(100);
                t
            })
            .collect();
        view.inventory.replace(1, terminals);
        view
    }

    /// Bounded `next` walk to a terminal row — never an open-ended loop.
    fn select(sb: &mut Sidebar, address: &str) {
        for _ in 0..sb.rows().len() + 1 {
            if sb.selected_terminal() == Some(address) {
                return;
            }
            sb.next();
        }
        panic!("{address} not reachable");
    }

    #[test]
    fn selection_follows_address_when_its_row_moves() {
        let mut sb = Sidebar::new();
        sb.update(&view_with(&["t1", "t3", "t9"]));
        select(&mut sb, "t3");
        // A new terminal inserted before it pushes the row down; the
        // address identity follows the move.
        let before = sb.selected_index().unwrap();
        sb.update(&view_with(&["t1", "t2", "t3", "t9"]));
        assert_eq!(sb.selected_terminal(), Some("t3"));
        assert!(sb.selected_index().unwrap() > before);
    }

    #[test]
    fn removal_falls_back_deterministically() {
        let mut sb = Sidebar::new();
        sb.update(&view_with(&["t1", "t3", "t9"]));
        select(&mut sb, "t3");
        let ix = sb.selected_index().unwrap();
        sb.update(&view_with(&["t1", "t9"]));
        assert_eq!(sb.selected_index().unwrap(), ix);
    }

    #[test]
    fn empty_inventory_falls_back_to_the_machine_row() {
        let mut sb = Sidebar::new();
        sb.update(&view_with(&["t1"]));
        sb.update(&View::default());
        // The local machine heading always exists; the deterministic
        // fallback lands on it.
        assert_eq!(sb.selected(), Some(&RowId::Machine("local".into())));
    }
}
