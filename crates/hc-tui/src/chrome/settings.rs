// Derived from herdr src/client/shell/settings.rs and state.rs (Apache-2.0).
//
//! The Settings overlay's state model: section selection, per-section
//! choice indices and the original-theme restore snapshot. hc's core has
//! no integration methods, so `integrations` stays empty and the
//! Integrations section renders its unavailable state (parity.md).
use super::palette::Palette;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum SettingsSection {
    Theme,
    Indicators,
    Sound,
    Toast,
    Integrations,
    /// Backlot's own tab (S0) — studio mode only.
    Studio,
    /// Per-machine Agent Skill management — studio mode only.
    AgentSkill,
}

impl SettingsSection {
    pub(crate) const ALL: &[Self] = &[
        Self::Theme,
        Self::Indicators,
        Self::Sound,
        Self::Toast,
        Self::Integrations,
    ];

    /// The tabs this client shows: herdr's five always, `studio`
    /// appended in studio mode (D6 — plain stays cell-identical).
    pub(crate) fn visible(studio: bool) -> Vec<Self> {
        let mut sections = Self::ALL.to_vec();
        if studio {
            sections.push(Self::Studio);
            sections.push(Self::AgentSkill);
        }
        sections
    }

    pub(crate) fn label(self) -> &'static str {
        match self {
            Self::Theme => "theme",
            Self::Indicators => "indicators",
            Self::Sound => "sound",
            Self::Toast => "toasts",
            Self::Integrations => "integrations",
            Self::Studio => "studio",
            Self::AgentSkill => "agent skill",
        }
    }
}

/// The studio tab's live values — a snapshot the overlay refreshes
/// after each write.
#[derive(Debug, Clone)]
pub(crate) struct StudioView {
    /// The call sheet's start policy.
    pub start: crate::panel::CallSheetStart,
    /// The direct call-sheet key (`alt+y`) bound.
    pub alt_y: bool,
    /// `[dock] width` — the expanded dock's columns.
    pub dock_width: u16,
    /// `runner · model`, or `None` while `[hollywood]` is unset.
    pub overseer: Option<String>,
    /// `not configured · backlot hollywood setup …` — shown while the
    /// overseer is unset; the row is read-only, Enter does nothing.
    pub setup: Option<String>,
}

/// The studio tab's row ids — its four rows are not one choice list,
/// so the renderer and the key router share these constants.
pub(crate) const STUDIO_START: usize = 0;
pub(crate) const STUDIO_ALT_Y: usize = 1;
pub(crate) const STUDIO_DOCK_WIDTH: usize = 2;
pub(crate) const STUDIO_OVERSEER: usize = 3;
pub(crate) const STUDIO_ROWS: usize = 4;

/// One agent row in the Agent Skill settings tab.
#[derive(Debug, Clone)]
pub(crate) struct SkillAgentView {
    pub agent: String,
    pub dir: String,
    pub state: backlot_model::summary::SkillState,
}

/// One machine's published (or locally computed) skill state.
#[derive(Debug, Clone)]
pub(crate) struct SkillMachineView {
    pub machine: Option<String>,
    pub label: String,
    pub rows: Vec<SkillAgentView>,
    pub known: bool,
}

/// The flattened rows the Agent Skill tab renders and keys act on.
#[derive(Debug, Clone)]
pub(crate) enum SkillSettingsRow {
    AllMachines,
    Machine {
        machine: Option<String>,
        label: String,
        agents: Vec<String>,
    },
    UnknownMachine {
        label: String,
    },
    Agent {
        machine: Option<String>,
        agent: String,
        dir: String,
        state: backlot_model::summary::SkillState,
    },
}

pub(crate) fn skill_rows(settings: &SettingsOverlay) -> Vec<SkillSettingsRow> {
    use backlot_model::summary::SkillState;
    let mut out = if settings.agent_skill_filter.is_some() {
        Vec::new()
    } else {
        vec![SkillSettingsRow::AllMachines]
    };
    for machine in &settings.agent_skill {
        if settings
            .agent_skill_filter
            .as_ref()
            .is_some_and(|wanted| wanted != &machine.machine)
        {
            continue;
        }
        if !machine.known {
            out.push(SkillSettingsRow::UnknownMachine {
                label: machine.label.clone(),
            });
            continue;
        }
        let agents = machine
            .rows
            .iter()
            .filter(|row| matches!(row.state, SkillState::Missing | SkillState::Stale))
            .map(|row| row.agent.clone())
            .collect::<Vec<_>>();
        out.push(SkillSettingsRow::Machine {
            machine: machine.machine.clone(),
            label: machine.label.clone(),
            agents,
        });
        for row in &machine.rows {
            out.push(SkillSettingsRow::Agent {
                machine: machine.machine.clone(),
                agent: row.agent.clone(),
                dir: row.dir.clone(),
                state: row.state,
            });
        }
    }
    out
}

/// The reference `api::schema::IntegrationState`. hc's core publishes no
/// integration rows; the enum survives so the render port stays verbatim.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum IntegrationState {
    #[allow(dead_code)]
    Current,
    Outdated,
    NotInstalled,
}

/// The reference `api::schema::IntegrationInfo` row; never populated.
#[derive(Debug)]
pub(crate) struct IntegrationInfo {
    pub state: IntegrationState,
    pub available: bool,
    #[allow(dead_code)]
    pub label: &'static str,
}

#[derive(Debug)]
pub(crate) struct SettingsOverlay {
    pub section: SettingsSection,
    pub selected: usize,
    /// The tabs the overlay shows — `ALL` plus `studio` in studio
    /// mode, kept here so plain mode's list is cell-identical.
    pub sections: Vec<SettingsSection>,
    /// The studio tab's values — `None` outside studio mode.
    pub studio: Option<StudioView>,
    /// Agent Skill rows by machine; empty only before studio data arrives.
    pub agent_skill: Vec<SkillMachineView>,
    /// `Some(machine)` when opened from a machine row menu.
    pub agent_skill_filter: Option<Option<String>>,
    /// A foreign row's first Enter — a second Enter is the destructive replace.
    pub skill_confirm: Option<(Option<String>, String)>,
    pub original_theme_name: String,
    /// The theme list shown: the reference's, or backlot's own.
    pub theme_names: &'static [&'static str],
    pub original_palette: Palette,
    /// Always empty in hc-tui — there is no `integration.list` method to
    /// populate it; the section renders "no integration targets available".
    pub integrations: Vec<IntegrationInfo>,
    pub loading_integrations: bool,
    pub installing_integrations: bool,
}

pub(crate) fn normalized_theme_name(name: &str) -> String {
    let normalized = name.to_lowercase().replace([' ', '_'], "-");
    crate::config::canonical_theme_name(&normalized)
        .map(str::to_owned)
        .unwrap_or(normalized)
}

pub(crate) fn theme_index(names: &[&str], name: &str) -> usize {
    let normalized = normalized_theme_name(name);
    names
        .iter()
        .position(|candidate| normalized_theme_name(candidate) == normalized)
        .unwrap_or(0)
}

pub(crate) fn indicator_index(style: crate::config::StatusIndicatorStyle) -> usize {
    usize::from(style == crate::config::StatusIndicatorStyle::Symbols)
}

pub(crate) fn toast_index(delivery: crate::config::ToastDelivery) -> usize {
    match delivery {
        crate::config::ToastDelivery::Off => 0,
        crate::config::ToastDelivery::Herdr => 1,
        crate::config::ToastDelivery::Terminal => 2,
        crate::config::ToastDelivery::System => 3,
    }
}

pub(crate) fn integration_needs_install(info: &IntegrationInfo) -> bool {
    info.state == IntegrationState::Outdated
        || info.available && info.state == IntegrationState::NotInstalled
}
