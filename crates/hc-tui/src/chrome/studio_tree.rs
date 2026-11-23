//! The machines tree's studio rows in studio mode (W1).
//!
//! A machine's sidebar rows are its herdr spaces — one per live studio
//! whose terminals exist. A studio that exists only in the ledger (its
//! stage is down, or the machine published no document) has no such
//! space, and a finished studio's space is gone entirely. This module
//! builds the extra rows the tree draws for those studios:
//!
//! - a dim `stage down` studio row that starts the machine's stage;
//! - a folded `finished (n)` group under the machine's live rows, whose
//!   unfolded rows open the finished-studio menu;
//! - one `unreachable` line for a machine that cannot be reached.
//!
//! Pure data: the app merges the summary and the ledger list, the tree
//! lays them out in sidebar order.

/// A studio row's state.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum StudioKind {
    /// Live, with no sidebar space — dim, but the studio exists.
    Live,
    /// The machine's stage is down: `stage down`, Enter starts it.
    StageDown,
}

/// Every machine's extra rows, keyed like the sidebar's endpoints.
#[derive(Debug, Default)]
pub(crate) struct Tree {
    pub(crate) rows: std::collections::BTreeMap<super::snapshot::ClientEndpointId, Vec<Row>>,
}

impl Tree {
    pub(crate) fn get(&self, endpoint: &super::snapshot::ClientEndpointId) -> &[Row] {
        self.rows.get(endpoint).map(Vec::as_slice).unwrap_or(&[])
    }
}

/// One extra row under a machine.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Row {
    /// A ledger studio without a sidebar space.
    Studio { id: String, kind: StudioKind },
    /// `▸ finished (n)` — `open` draws `▾` and the rows after it.
    FinishedFold { count: usize, open: bool },
    /// One finished studio, drawn only while the fold is open.
    Finished { id: String },
    /// `× unreachable` — the machine cannot answer at all.
    Unreachable,
}

/// One studio the app knows for a machine.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Studio<'a> {
    pub(crate) id: &'a str,
    pub(crate) state: StudioState,
}

impl<'a> Studio<'a> {
    pub(crate) fn live(id: &'a str) -> Studio<'a> {
        Studio {
            id,
            state: StudioState::Live,
        }
    }

    pub(crate) fn finished(id: &'a str) -> Studio<'a> {
        Studio {
            id,
            state: StudioState::Finished,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum StudioState {
    Live,
    Finished,
}

/// What one machine's tree section is built from.
pub(crate) struct Machine<'a> {
    pub(crate) reachable: bool,
    pub(crate) stage_running: bool,
    /// The labels of the machine's sidebar spaces — a studio with one
    /// is already drawn by the workspace row, so it adds no extra row.
    pub(crate) space_labels: &'a [&'a str],
    /// Every studio the summary and the ledger name, summary first.
    pub(crate) studios: &'a [Studio<'a>],
    /// The `finished (n)` group is unfolded for this machine.
    pub(crate) finished_open: bool,
}

/// The extra rows under `machine`, in draw order: the live studios with
/// no space row first, then the finished group.
pub(crate) fn rows(machine: &Machine<'_>) -> Vec<Row> {
    if !machine.reachable {
        return vec![Row::Unreachable];
    }
    let mut out = Vec::new();
    for studio in machine.studios {
        if studio.state == StudioState::Finished {
            continue;
        }
        if machine.space_labels.contains(&studio.id) {
            // The workspace row already lists it and focuses its person.
            continue;
        }
        out.push(Row::Studio {
            id: studio.id.to_owned(),
            kind: if machine.stage_running {
                StudioKind::Live
            } else {
                StudioKind::StageDown
            },
        });
    }
    let finished: Vec<&str> = machine
        .studios
        .iter()
        .filter(|studio| studio.state == StudioState::Finished)
        .map(|studio| studio.id)
        .collect();
    if !finished.is_empty() {
        out.push(Row::FinishedFold {
            count: finished.len(),
            open: machine.finished_open,
        });
        if machine.finished_open {
            out.extend(
                finished
                    .into_iter()
                    .map(|id| Row::Finished { id: id.to_owned() }),
            );
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    const SPACES: &[&str] = &["acme-shop"];

    fn machine<'a>(
        spaces: &'a [&'a str],
        studios: &'a [Studio<'a>],
        stage_running: bool,
        finished_open: bool,
    ) -> Machine<'a> {
        Machine {
            reachable: true,
            stage_running,
            space_labels: spaces,
            studios,
            finished_open,
        }
    }

    #[test]
    fn a_stage_down_studio_without_a_space_is_a_dim_row() {
        let studios = [Studio::live("beta-tools")];
        let rows = rows(&machine(&[], &studios, false, false));
        assert_eq!(
            rows,
            vec![Row::Studio {
                id: "beta-tools".into(),
                kind: StudioKind::StageDown
            }]
        );
    }

    #[test]
    fn a_live_studio_with_a_space_row_adds_nothing() {
        let studios = [Studio::live("acme-shop")];
        assert!(rows(&machine(SPACES, &studios, true, false)).is_empty());
    }

    #[test]
    fn the_finished_group_counts_and_unfolds() {
        let studios = [
            Studio::live("acme-shop"),
            Studio::finished("old-a"),
            Studio::finished("old-b"),
        ];
        // Live space rows are the workspace's; the fold is ours.
        let folded = rows(&machine(SPACES, &studios, true, false));
        assert_eq!(
            folded,
            vec![Row::FinishedFold {
                count: 2,
                open: false
            }]
        );
        let open = rows(&machine(SPACES, &studios, true, true));
        assert_eq!(
            open,
            vec![
                Row::FinishedFold {
                    count: 2,
                    open: true
                },
                Row::Finished { id: "old-a".into() },
                Row::Finished { id: "old-b".into() },
            ]
        );
    }

    #[test]
    fn a_finished_studio_never_doubles_as_a_live_row() {
        let studios = [Studio::finished("acme-shop")];
        assert_eq!(
            rows(&machine(SPACES, &studios, false, false)),
            vec![Row::FinishedFold {
                count: 1,
                open: false
            }]
        );
    }

    #[test]
    fn an_unreachable_machine_is_one_line() {
        let studios = [Studio::live("beta-tools"), Studio::finished("old")];
        let mut input = machine(&[], &studios, false, false);
        input.reachable = false;
        assert_eq!(rows(&input), vec![Row::Unreachable]);
    }
}
