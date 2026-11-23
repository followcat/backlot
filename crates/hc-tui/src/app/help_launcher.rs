//! The `Ctrl+B ?` launcher's App side (S5 §1, A1.1): the group
//! builder that turns the Call Sheet's waiting items into runnable
//! rows, the list's own keys (arrows move, Enter runs or focuses),
//! and the compact target picker. Every runnable press lands in
//! `run_studio_act` — the S3 activation arm — so a row's action
//! tracks, toasts and dialogs exactly like its menu.

use super::App;
use crate::help_launcher::{HelpGroup, HelpRow, Launch, Pick, Surface};
use crate::hollywood::{self, Wait};
use crate::panel::data;
use crate::studio::StudioKey;
use crate::studio_menu::{StudioAct, StudioMenu};

impl App {
    // ---------- the model ----------

    /// The launcher's hint for "open the Call Sheet, then press
    /// `<key>`" — the resolved toggle label, then the row's own key.
    fn sheet_hint(&self, key: &str) -> String {
        let keys = self.hollywood_key.unwrap_or_default();
        format!(
            "{} -> {key}",
            keys.label(&crate::config::format_prefix_combos(&self.live.prefix))
        )
    }

    /// The dock chord's label — `ctrl+b y` for the default config.
    fn dock_chord(&self) -> String {
        self.panel_key_combo
            .map(|combo| {
                format!(
                    "{} {}",
                    crate::config::format_prefix_combos(&self.live.prefix),
                    crate::config::format_key_combo(combo)
                )
            })
            .unwrap_or_else(|| "unset".to_owned())
    }

    /// The `waiting on you` group — every item the Call Sheet would
    /// list, each row carrying the `StudioMenu`/`StudioAct` pair the
    /// press runs. Order is the sheet's own urgency order.
    fn waiting_group(&self) -> HelpGroup {
        let machines = self.panel_summaries();
        let status = hollywood::status_of(&self.view);
        let local = crate::project::LOCAL_LABEL;
        let multi = data::spans_machines(&machines);
        let mut rows = Vec::new();
        for wait in hollywood::waiting_items(&machines, &status) {
            match &wait {
                Wait::Prompt(item) => {
                    let key = StudioKey {
                        machine: item.machine.map(str::to_string),
                        id: item.studio.id.clone(),
                    };
                    let menu =
                        StudioMenu::role(&key, item.role, !self.machine_reachable(item.machine));
                    let seq = item.role.attention_seq.unwrap_or(0);
                    let place = help_place(&key, local, multi);
                    // A runner that cannot answer its own prompt offers
                    // no Allow/Deny row here — the sheet row says to
                    // answer in its terminal.
                    if item.role.can_answer_prompt {
                        for (word, allow, k) in [("Allow", true, "y"), ("Deny", false, "d")] {
                            rows.push(HelpRow {
                                label: format!(
                                    "{word} prompt #{seq} · {place} · {}",
                                    item.role.name
                                ),
                                hint: self.sheet_hint(k),
                                glyph: "×",
                                launch: Launch::Act {
                                    menu: Box::new(menu.clone()),
                                    act: StudioAct::Decide(allow),
                                },
                            });
                        }
                    }
                }
                Wait::Stage(item) => {
                    let mut menu = StudioMenu::stage(item.offline);
                    menu.machine = item.machine.map(str::to_string);
                    let machine = hollywood::machine_label(item.machine, local, multi);
                    let machine = if machine.is_empty() {
                        local.to_string()
                    } else {
                        machine.to_string()
                    };
                    if item.offline {
                        rows.push(HelpRow {
                            label: format!("Check machine {machine}"),
                            hint: self.hollywood_key_label(),
                            glyph: "×",
                            launch: Launch::Surface(Surface::CallSheet),
                        });
                    } else {
                        rows.push(HelpRow {
                            label: format!("Start stage · {machine}"),
                            hint: self.sheet_hint("s"),
                            glyph: "◌",
                            launch: Launch::Act {
                                menu: Box::new(menu),
                                act: StudioAct::StageStart,
                            },
                        });
                    }
                }
                Wait::Role(item) => {
                    let place = format!(
                        "{} · {}",
                        item.studio.id,
                        hollywood::machine_label(item.machine, local, multi)
                    );
                    rows.push(HelpRow {
                        label: format!(
                            "{} · {place} · {}",
                            if item.asks { "Question" } else { "Needs you" },
                            item.role.name
                        ),
                        hint: self.sheet_hint("↵"),
                        glyph: if item.asks { "?" } else { "×" },
                        launch: Launch::Surface(Surface::CallSheet),
                    });
                }
                Wait::Hollywood(hollywood) if hollywood.attention_user => rows.push(HelpRow {
                    label: format!(
                        "Hollywood asks: {}",
                        hollywood
                            .attention_detail
                            .as_deref()
                            .or(hollywood.attention.as_deref())
                            .unwrap_or("permission prompt")
                    ),
                    hint: self.sheet_hint("↵"),
                    glyph: "⚠",
                    launch: Launch::Surface(Surface::CallSheet),
                }),
                Wait::Hollywood(_) => rows.push(HelpRow {
                    label: "Hollywood is blocked".to_string(),
                    hint: self.sheet_hint("↵"),
                    glyph: "!",
                    launch: Launch::Surface(Surface::CallSheet),
                }),
                Wait::NoRequirement(item) => rows.push(HelpRow {
                    label: format!("New studio {} · tell the producer", item.studio.id),
                    hint: self.sheet_hint("↵"),
                    glyph: "✦",
                    launch: Launch::Surface(Surface::CallSheet),
                }),
                Wait::Card(card) => {
                    let menu = StudioMenu::card(&card.id);
                    rows.push(HelpRow {
                        label: format!("Apply card {}: {} · {}", card.id, card.action, card.studio),
                        hint: self.sheet_hint("a"),
                        glyph: "◆",
                        launch: Launch::Act {
                            menu: Box::new(menu.clone()),
                            act: StudioAct::CardApply,
                        },
                    });
                    rows.push(HelpRow {
                        label: format!("Cancel card {}…", card.id),
                        hint: self.sheet_hint("x"),
                        glyph: "◆",
                        launch: Launch::Act {
                            menu: Box::new(menu),
                            act: StudioAct::CardCancel,
                        },
                    });
                }
                Wait::Ready(item) => {
                    let key = StudioKey {
                        machine: item.machine.map(str::to_string),
                        id: item.studio.id.clone(),
                    };
                    let menu = StudioMenu::studio(
                        &key,
                        item.studio,
                        !self.machine_stage_running(item.machine),
                        !self.machine_reachable(item.machine),
                    );
                    rows.push(HelpRow {
                        label: format!("Finish studio · {}", help_place(&key, local, multi)),
                        hint: self.sheet_hint("f"),
                        glyph: "✓",
                        launch: Launch::Act {
                            menu: Box::new(menu),
                            act: StudioAct::Finish,
                        },
                    });
                }
            }
        }
        HelpGroup {
            title: "waiting on you (↵ runs immediately)".to_owned(),
            rows,
        }
    }

    /// The `alt+y / ctrl+b i`-style toggle label alone.
    fn hollywood_key_label(&self) -> String {
        self.hollywood_key
            .unwrap_or_default()
            .label(&crate::config::format_prefix_combos(&self.live.prefix))
    }

    /// U6c's `machines` group — a row only where the machine has a
    /// valid action: `Upgrade backlot to v<latest>` behind, `Install
    /// backlot` missing, `Restart the stage` while its stage runs.
    /// A down stage is the waiting group's `Start stage` already.
    fn machines_group(&self) -> HelpGroup {
        let machines = self.panel_summaries();
        let local = crate::project::LOCAL_LABEL;
        let multi = data::spans_machines(&machines);
        let latest = self.latest_release();
        let mut rows = Vec::new();
        for endpoint in &self.projection.endpoints {
            let menu = self.machine_menu(&endpoint.endpoint_id);
            let machine = menu.machine.clone();
            let name = hollywood::machine_label(machine.as_deref(), local, multi);
            let name = if name.is_empty() { local } else { name };
            let (label, act) = match &menu.backlot {
                Some(crate::studio_menu::MenuBacklot::Installed(installed))
                    if latest
                        .as_deref()
                        .is_some_and(|l| crate::machine_versions::behind(installed, l)) =>
                {
                    (
                        format!(
                            "Upgrade backlot to v{} · {name}",
                            latest.clone().unwrap_or_default()
                        ),
                        StudioAct::MachineInstall,
                    )
                }
                Some(crate::studio_menu::MenuBacklot::Missing) => (
                    format!("Install backlot · {name}"),
                    StudioAct::MachineInstall,
                ),
                Some(crate::studio_menu::MenuBacklot::Installed(_))
                    if !menu.stage_down && !menu.offline =>
                {
                    (
                        format!("Restart the stage · {name}"),
                        StudioAct::StageRestart,
                    )
                }
                _ => continue,
            };
            rows.push(HelpRow {
                label,
                hint: "run ↵".to_owned(),
                glyph: "",
                launch: Launch::Act {
                    menu: Box::new(menu),
                    act,
                },
            });
        }
        HelpGroup {
            title: "machines".to_owned(),
            rows,
        }
    }

    /// The four studio keys, leading the `studio & task actions`
    /// group (R3-N6): each row names its combo — `unset` when the
    /// `[keys]` entry is `""` — and the herdr action it shadows, and
    /// Enter runs the navigation itself.
    fn studio_key_rows(&self) -> Vec<HelpRow> {
        let keys = &self.panel_config.keys;
        [
            (
                crate::panel::StudioNav::PreviousStudio,
                keys.previous_studio,
                "previous studio",
            ),
            (
                crate::panel::StudioNav::NextStudio,
                keys.next_studio,
                "next studio",
            ),
            (
                crate::panel::StudioNav::PreviousRole,
                keys.previous_role,
                "previous role",
            ),
            (
                crate::panel::StudioNav::NextRole,
                keys.next_role,
                "next role",
            ),
        ]
        .into_iter()
        .map(|(nav, combo, label)| HelpRow {
            label: format!(
                "{}{}",
                label,
                crate::keybind_help::studio_key_shadow(&self.live.keybinds, combo)
            ),
            hint: combo
                .map(crate::config::format_key_combo)
                .unwrap_or_else(|| "unset".to_owned()),
            glyph: "",
            launch: combo.map_or(Launch::Close, |_| Launch::Nav(nav)),
        })
        .collect()
    }

    /// The `studio & task actions` group — the four studio keys,
    /// then the surface rows, then the `…` pickers. A picker with
    /// no targets at all leaves no row: the overlay never offers a
    /// no-op press.
    fn actions_group(&self) -> HelpGroup {
        let prefix = crate::config::format_prefix_combos(&self.live.prefix);
        let dock = self.dock_chord();
        let mut rows = self.studio_key_rows();
        rows.extend([
            HelpRow {
                label: "Open Call Sheet (prompts, cards, tell hollywood)".to_owned(),
                hint: self.hollywood_key_label(),
                glyph: "",
                launch: Launch::Surface(Surface::CallSheet),
            },
            HelpRow {
                label: "Toggle Unified Studio Dock".to_owned(),
                hint: dock.clone(),
                glyph: "",
                launch: Launch::Surface(Surface::Dock),
            },
            HelpRow {
                label: "Context menu for focused role / studio".to_owned(),
                hint: format!("{prefix} m / m"),
                glyph: "",
                launch: Launch::Surface(Surface::PaneMenu),
            },
            HelpRow {
                label: "New studio on machine…".to_owned(),
                hint: "run ↵".to_owned(),
                glyph: "",
                launch: Launch::Pick(Pick::Machine),
            },
        ]);
        if !self.picker_rows(Pick::Task).is_empty() {
            rows.push(HelpRow {
                label: "Inspect task deliveries, checks & reviews…".to_owned(),
                hint: format!("{dock} -> v"),
                glyph: "",
                launch: Launch::Pick(Pick::Task),
            });
        }
        if !self.picker_rows(Pick::Studio(StudioAct::Revise)).is_empty() {
            rows.push(HelpRow {
                label: "Revise studio requirement…".to_owned(),
                hint: "run ↵".to_owned(),
                glyph: "",
                launch: Launch::Pick(Pick::Studio(StudioAct::Revise)),
            });
        }
        if !self.picker_rows(Pick::TaskVerb).is_empty() {
            rows.push(HelpRow {
                label: "Pause / resume / abandon / take over task…".to_owned(),
                hint: "run ↵".to_owned(),
                glyph: "",
                launch: Launch::Pick(Pick::TaskVerb),
            });
        }
        if !self.picker_rows(Pick::PersonReopen).is_empty() {
            rows.push(HelpRow {
                label: "Reopen closed person…".to_owned(),
                hint: format!("{dock} -> o"),
                glyph: "",
                launch: Launch::Pick(Pick::PersonReopen),
            });
        }
        HelpGroup {
            title: "studio & task actions".to_owned(),
            rows,
        }
    }

    /// The `hollywood` group — the `tell hollywood` input the bar's
    /// `i` opens.
    fn hollywood_group(&self) -> HelpGroup {
        HelpGroup {
            title: "hollywood".to_owned(),
            rows: vec![HelpRow {
                label: "Tell hollywood what you need…".to_owned(),
                hint: self.sheet_hint("i"),
                glyph: "",
                launch: Launch::Surface(Surface::CallSheetTell),
            }],
        }
    }

    /// The full launcher list: the three studio groups, then the
    /// studio surfaces' keybind references (`studio`, `call sheet
    /// (open)`, … — S0's sections, minus the `backlot` chords the
    /// actions group already names and the pinned `glyph legend`),
    /// then herdr's own sections as close-on-Enter reference rows.
    pub(crate) fn help_groups(&self) -> Vec<HelpGroup> {
        let mut groups = vec![
            self.waiting_group(),
            self.actions_group(),
            self.hollywood_group(),
            self.machines_group(),
        ];
        // U6c: an empty machines section (a single local endpoint,
        // nothing to install or restart) never lists.
        groups.retain(|g| g.title != "machines" || !g.rows.is_empty());
        let reference = crate::keybind_help::backlot_help_groups(
            &self.live.keybinds,
            &self.live.prefix,
            &self.panel_config,
            self.hollywood_key.unwrap_or_default(),
            self.panel_key_combo,
        );
        groups.extend(
            reference
                .into_iter()
                .filter(|(title, _)| {
                    // `backlot`'s chords are named by the actions
                    // group already, the `studio` rows lead it
                    // (R3-N6) and the legend is pinned by the
                    // renderer — none list again down here.
                    !matches!(*title, "backlot" | "studio" | "glyph legend")
                })
                .map(|(title, entries)| HelpGroup {
                    title: title.to_owned(),
                    rows: entries
                        .into_iter()
                        .map(|(key, label)| HelpRow {
                            label: label.into_owned(),
                            hint: key,
                            glyph: "",
                            launch: Launch::Close,
                        })
                        .collect(),
                }),
        );
        for (title, entries) in
            crate::keybind_help::keybind_help_groups(&self.live.keybinds, &self.live.prefix)
        {
            groups.push(HelpGroup {
                title: title.to_owned(),
                rows: entries
                    .into_iter()
                    .map(|(key, label)| HelpRow {
                        label: label.into_owned(),
                        hint: key,
                        glyph: "",
                        launch: Launch::Close,
                    })
                    .collect(),
            });
        }
        groups
    }

    /// The two pinned legend rows — the `glyph legend` section the
    /// classic overlay draws above its footer.
    pub(crate) fn help_legend(&self) -> Vec<(String, String)> {
        crate::keybind_help::backlot_help_groups(
            &self.live.keybinds,
            &self.live.prefix,
            &self.panel_config,
            self.hollywood_key.unwrap_or_default(),
            self.panel_key_combo,
        )
        .into_iter()
        .find(|(title, _)| *title == "glyph legend")
        .map(|(_, entries)| {
            entries
                .into_iter()
                .map(|(key, label)| (key, label.into_owned()))
                .collect()
        })
        .unwrap_or_default()
    }

    /// The query-filtered groups — a row matches on label or hint;
    /// a group whose title matches keeps all its rows.
    pub(crate) fn help_filtered_groups(&self) -> Vec<HelpGroup> {
        let groups = self.help_groups();
        let query = self.help_state.query.as_str().trim().to_lowercase();
        if query.is_empty() {
            return groups;
        }
        groups
            .into_iter()
            .filter_map(|mut group| {
                if group.title.to_lowercase().contains(&query) {
                    return Some(group);
                }
                group.rows.retain(|row| {
                    row.label.to_lowercase().contains(&query)
                        || row.hint.to_lowercase().contains(&query)
                });
                (!group.rows.is_empty()).then_some(group)
            })
            .collect()
    }

    /// The filtered row list as flat references — `selected` indexes
    /// into it.
    pub(super) fn help_rows(&self) -> Vec<HelpRow> {
        self.help_filtered_groups()
            .into_iter()
            .flat_map(|g| g.rows)
            .collect()
    }
}

/// `acme-shop · Local` — the studio id plus its machine label when
/// studios span machines.
pub(super) fn help_place(key: &StudioKey, local: &str, multi: bool) -> String {
    match (key.machine.as_deref(), multi) {
        (Some(machine), _) => format!("{} · {machine}", key.id),
        (None, true) => format!("{} · {local}", key.id),
        (None, false) => key.id.clone(),
    }
}
