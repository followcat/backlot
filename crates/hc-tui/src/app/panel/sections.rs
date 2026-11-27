//! Lot Dock `Tab` / `Shift+Tab` section cycling (`Header -> People ->
//! Tasks -> Exchange -> Input`), skipping empty sections, crossing into
//! adjacent studio blocks (compact/stacked count as header-only), and
//! wrapping at both ends.

use super::App;
use crate::panel::{Section, Target};
use crate::studio::StudioKey;

const SECTIONS: [Section; 5] = [
    Section::Header,
    Section::People,
    Section::Tasks,
    Section::Exchange,
    Section::Input,
];

struct Entry {
    index: usize,
    target: Target,
    studio: StudioKey,
    section: Section,
}

fn collect_entries(rows: &[crate::panel::Row<'_>]) -> (Vec<Entry>, Vec<(StudioKey, Target)>) {
    let mut entries = Vec::new();
    let mut studios: Vec<(StudioKey, Target)> = Vec::new();
    for (index, row) in rows.iter().enumerate() {
        let Some((studio, section)) = row.section_key() else {
            continue;
        };
        let target = row.target();
        if !studios.iter().any(|(k, _)| k == &studio) {
            studios.push((studio.clone(), target.clone()));
        }
        entries.push(Entry {
            index,
            target,
            studio,
            section,
        });
    }
    (entries, studios)
}

fn first_in_section<'a>(
    entries: &'a [Entry],
    studio: &StudioKey,
    section: Section,
) -> Option<&'a Entry> {
    entries
        .iter()
        .find(|e| &e.studio == studio && e.section == section)
}

impl App {
    /// Jump to the first selectable row of the next (`Tab`) or previous
    /// (`Shift+Tab`) non-empty section, wrapping across studios.
    pub(super) fn panel_tab_section(&mut self, forward: bool) {
        let machines = self.panel_summaries();
        let body_h = self.panel_hits.body.height;
        let mut rows = self.panel_rows_at(&machines, body_h);
        if rows.len() <= 1 {
            return;
        }
        let (entries, studios) = collect_entries(&rows);
        if studios.is_empty() {
            return;
        }

        let targets: Vec<Target> = rows.iter().map(|r| r.target()).collect();
        let cur_idx = self.panel_selected_index(&targets);
        let current_loc = self
            .panel
            .selected
            .as_ref()
            .and_then(|_| rows.get(cur_idx))
            .and_then(|r| r.section_key());

        let dest_key = if let Some((cur_key, cur_sec)) = current_loc {
            let candidate = if forward {
                SECTIONS
                    .iter()
                    .copied()
                    .filter(|&s| s > cur_sec)
                    .find_map(|s| first_in_section(&entries, &cur_key, s))
            } else {
                SECTIONS
                    .iter()
                    .rev()
                    .copied()
                    .filter(|&s| s < cur_sec)
                    .find_map(|s| first_in_section(&entries, &cur_key, s))
            };
            if let Some(next) = candidate {
                self.finish_tab_jump(&rows, next.index, next.target.clone(), &next.studio);
                return;
            }
            let pos = studios.iter().position(|(k, _)| k == &cur_key).unwrap_or(0);
            let next_pos = if forward {
                (pos + 1) % studios.len()
            } else {
                (pos + studios.len() - 1) % studios.len()
            };
            studios[next_pos].0.clone()
        } else if forward {
            studios[0].0.clone()
        } else {
            studios.last().expect("non-empty studios").0.clone()
        };

        let header_target = studios
            .iter()
            .find(|(k, _)| k == &dest_key)
            .map(|(_, t)| t.clone())
            .unwrap_or_else(|| Target::Studio(dest_key.clone()));

        if forward {
            // Crossing to another studio's header: unfold a folded card just
            // as `Down` onto its header does, keeping Compact/Stacked 1-line.
            self.panel.selected = Some(header_target.clone());
            rows = self.panel_rows_at(&machines, body_h);
            let rebuilt_targets: Vec<Target> = rows.iter().map(|r| r.target()).collect();
            let index = rebuilt_targets
                .iter()
                .position(|t| *t == header_target)
                .or_else(|| match &header_target {
                    Target::Block(k) => rebuilt_targets
                        .iter()
                        .position(|t| *t == Target::Studio(k.clone())),
                    _ => None,
                })
                .or_else(|| {
                    rebuilt_targets
                        .iter()
                        .position(|t| t.studio() == Some(&dest_key))
                })
                .unwrap_or(0);
            let target = rebuilt_targets.get(index).cloned().unwrap_or(header_target);
            self.finish_tab_jump(&rows, index, target, &dest_key);
        } else {
            // Crossing backward to `dest_key`'s last non-empty section: if
            // `dest_key` is Folded (`Target::Studio`), selecting it unfolds its
            // full block so its `›` input (or last populated section) resolves;
            // Compact/Stacked/Strip rows stay header-only (`Header`).
            if matches!(header_target, Target::Studio(_)) {
                self.panel.selected = Some(header_target.clone());
                rows = self.panel_rows_at(&machines, body_h);
            }
            let (rebuilt_entries, _) = collect_entries(&rows);
            let last_entry = SECTIONS
                .iter()
                .rev()
                .copied()
                .find_map(|s| first_in_section(&rebuilt_entries, &dest_key, s));
            if let Some(entry) = last_entry {
                self.finish_tab_jump(&rows, entry.index, entry.target.clone(), &entry.studio);
            }
        }
    }

    fn finish_tab_jump(
        &mut self,
        rows: &[crate::panel::Row<'_>],
        mut index: usize,
        target: Target,
        studio: &StudioKey,
    ) {
        let targets: Vec<Target> = rows.iter().map(|r| r.target()).collect();
        while index > 0 && targets.get(index - 1) == targets.get(index) {
            index -= 1;
        }
        self.panel.selected = Some(target);
        self.panel.selected_row = Some(index);
        self.panel.finish_confirm = None;
        self.panel.editing = false;
        self.panel.reset_history_nav();
        self.set_workspace_focus(crate::focus::WorkspaceRegion::Lot(
            crate::focus::LotLevel::InsideStudio(studio.clone()),
        ));
        self.reveal_panel_line(rows, index);
    }
}
