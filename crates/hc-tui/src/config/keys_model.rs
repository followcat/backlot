// Derived from herdr src/config/model.rs (Apache-2.0).
use super::*;

/// Sidebar row composition (`ui.sidebar.agents` / `ui.sidebar.spaces`).
/// Re-exported from the `sidebar` module.
#[derive(Debug, Clone, Deserialize)]
#[serde(default)]
pub struct CommandKeybindConfig {
    /// Key that runs a command. `prefix+g` for prefix mode or a modified chord for direct mode.
    pub key: BindingConfig,
    /// Command executed either in the background shell or inside a pane.
    pub command: String,
    /// Command execution mode. Default: "shell".
    #[serde(rename = "type")]
    pub action_type: CommandKeybindType,
    /// Optional user-defined description for this custom command.
    pub description: Option<String>,
    /// Popup geometry when type = "popup" (cells or percentage strings).
    pub width: Option<toml::Value>,
    pub height: Option<toml::Value>,
}

impl CommandKeybindConfig {}

impl Default for CommandKeybindConfig {
    fn default() -> Self {
        Self {
            key: BindingConfig::empty(),
            command: String::new(),
            action_type: CommandKeybindType::Shell,
            description: None,
            width: None,
            height: None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum CommandKeybindType {
    #[default]
    Shell,
    Pane,
    Popup,
    PluginAction,
}

/// Optional indexed shortcuts expanded over number keys 1-9.
#[derive(Debug, Default, Deserialize)]
#[serde(default)]
pub struct IndexedKeysConfig {
    /// Modifier combo for tab shortcuts 1-9. Unset by default.
    pub tabs: String,
    /// Modifier combo for workspace shortcuts 1-9. Unset by default.
    pub workspaces: String,
    /// Modifier combo for agent shortcuts 1-9. Unset by default.
    pub agents: String,
}

/// The raw overlay deserialized from `[keys]`: every field optional so a
/// merge over defaults can mark which fields the user configured.
/// Derived from herdr src/config/model.rs `KeysConfigOverlay` (Apache-2.0).
#[derive(Debug, Default, Deserialize)]
#[serde(default)]
pub struct KeysConfigOverlay {
    pub prefix: Option<BindingConfig>,
    pub extra_prefixes: Option<BindingConfig>,
    pub help: Option<BindingConfig>,
    pub settings: Option<BindingConfig>,
    pub new_workspace: Option<BindingConfig>,
    pub new_worktree: Option<BindingConfig>,
    pub open_worktree: Option<BindingConfig>,
    pub remove_worktree: Option<BindingConfig>,
    pub rename_workspace: Option<BindingConfig>,
    pub close_workspace: Option<BindingConfig>,
    pub workspace_picker: Option<BindingConfig>,
    pub goto: Option<BindingConfig>,
    pub navigate_workspace_up: Option<BindingConfig>,
    pub navigate_workspace_down: Option<BindingConfig>,
    pub navigate_pane_left: Option<BindingConfig>,
    pub navigate_pane_down: Option<BindingConfig>,
    pub navigate_pane_up: Option<BindingConfig>,
    pub navigate_pane_right: Option<BindingConfig>,
    pub detach: Option<BindingConfig>,
    pub reload_config: Option<BindingConfig>,
    pub open_notification_target: Option<BindingConfig>,
    pub previous_workspace: Option<BindingConfig>,
    pub next_workspace: Option<BindingConfig>,
    pub previous_agent: Option<BindingConfig>,
    pub next_agent: Option<BindingConfig>,
    pub focus_agent: Option<BindingConfig>,
    pub remote_image_paste: Option<String>,
    pub new_tab: Option<BindingConfig>,
    pub rename_tab: Option<BindingConfig>,
    pub previous_tab: Option<BindingConfig>,
    pub next_tab: Option<BindingConfig>,
    pub move_tab_previous: Option<BindingConfig>,
    pub move_tab_next: Option<BindingConfig>,
    pub switch_tab: Option<BindingConfig>,
    pub switch_workspace: Option<BindingConfig>,
    pub close_tab: Option<BindingConfig>,
    pub rename_pane: Option<BindingConfig>,
    pub edit_scrollback: Option<BindingConfig>,
    pub clear_pane: Option<BindingConfig>,
    pub copy_mode: Option<BindingConfig>,
    pub focus_pane_left: Option<BindingConfig>,
    pub focus_pane_down: Option<BindingConfig>,
    pub focus_pane_up: Option<BindingConfig>,
    pub focus_pane_right: Option<BindingConfig>,
    pub swap_pane_left: Option<BindingConfig>,
    pub swap_pane_down: Option<BindingConfig>,
    pub swap_pane_up: Option<BindingConfig>,
    pub swap_pane_right: Option<BindingConfig>,
    pub cycle_pane_next: Option<BindingConfig>,
    pub cycle_pane_previous: Option<BindingConfig>,
    pub last_pane: Option<BindingConfig>,
    pub split_vertical: Option<BindingConfig>,
    pub split_horizontal: Option<BindingConfig>,
    pub close_pane: Option<BindingConfig>,
    pub zoom: Option<BindingConfig>,
    pub resize_mode: Option<BindingConfig>,
    pub resize_pane_left: Option<BindingConfig>,
    pub resize_pane_down: Option<BindingConfig>,
    pub resize_pane_up: Option<BindingConfig>,
    pub resize_pane_right: Option<BindingConfig>,
    pub toggle_sidebar: Option<BindingConfig>,
    pub indexed: Option<IndexedKeysConfig>,
    pub command: Option<Vec<CommandKeybindConfig>>,
}

#[derive(Debug)]
pub struct KeysConfig {
    /// `[keys]` fields the user file actually set; user bindings register
    /// before defaults so they displace colliding builtins.
    pub(crate) user_fields: std::collections::BTreeSet<&'static str>,
    pub prefix: BindingConfig,
    pub help: BindingConfig,
    pub settings: BindingConfig,
    pub new_workspace: BindingConfig,
    pub new_worktree: BindingConfig,
    pub open_worktree: BindingConfig,
    pub remove_worktree: BindingConfig,
    pub rename_workspace: BindingConfig,
    pub close_workspace: BindingConfig,
    pub workspace_picker: BindingConfig,
    pub goto: BindingConfig,
    pub navigate_workspace_up: BindingConfig,
    pub navigate_workspace_down: BindingConfig,
    pub navigate_pane_left: BindingConfig,
    pub navigate_pane_down: BindingConfig,
    pub navigate_pane_up: BindingConfig,
    pub navigate_pane_right: BindingConfig,
    pub detach: BindingConfig,
    pub reload_config: BindingConfig,
    pub open_notification_target: BindingConfig,
    pub previous_workspace: BindingConfig,
    pub next_workspace: BindingConfig,
    pub previous_agent: BindingConfig,
    pub next_agent: BindingConfig,
    pub focus_agent: BindingConfig,
    pub remote_image_paste: String,
    pub new_tab: BindingConfig,
    pub rename_tab: BindingConfig,
    pub previous_tab: BindingConfig,
    pub next_tab: BindingConfig,
    pub move_tab_previous: BindingConfig,
    pub move_tab_next: BindingConfig,
    pub switch_tab: BindingConfig,
    pub switch_workspace: BindingConfig,
    pub close_tab: BindingConfig,
    pub rename_pane: BindingConfig,
    pub edit_scrollback: BindingConfig,
    pub clear_pane: BindingConfig,
    pub copy_mode: BindingConfig,
    pub focus_pane_left: BindingConfig,
    pub focus_pane_down: BindingConfig,
    pub focus_pane_up: BindingConfig,
    pub focus_pane_right: BindingConfig,
    pub swap_pane_left: BindingConfig,
    pub swap_pane_down: BindingConfig,
    pub swap_pane_up: BindingConfig,
    pub swap_pane_right: BindingConfig,
    pub cycle_pane_next: BindingConfig,
    pub cycle_pane_previous: BindingConfig,
    pub last_pane: BindingConfig,
    pub split_vertical: BindingConfig,
    pub split_horizontal: BindingConfig,
    pub close_pane: BindingConfig,
    pub zoom: BindingConfig,
    pub resize_mode: BindingConfig,
    pub resize_pane_left: BindingConfig,
    pub resize_pane_down: BindingConfig,
    pub resize_pane_up: BindingConfig,
    pub resize_pane_right: BindingConfig,
    pub toggle_sidebar: BindingConfig,
    pub indexed: IndexedKeysConfig,
    /// Prefix-mode custom command bindings.
    pub command: Vec<CommandKeybindConfig>,
}

impl Default for KeysConfig {
    fn default() -> Self {
        Self {
            user_fields: std::collections::BTreeSet::new(),
            prefix: BindingConfig::one("ctrl+b"),
            help: BindingConfig::one("prefix+?"),
            settings: BindingConfig::one("prefix+s"),
            new_workspace: BindingConfig::one("prefix+shift+n"),
            new_worktree: BindingConfig::one("prefix+shift+g"),
            open_worktree: BindingConfig::empty(),
            remove_worktree: BindingConfig::empty(),
            rename_workspace: BindingConfig::one("prefix+shift+w"),
            close_workspace: BindingConfig::one("prefix+shift+d"),
            workspace_picker: BindingConfig::one("prefix+w"),
            goto: BindingConfig::one("prefix+g"),
            navigate_workspace_up: BindingConfig::one("up"),
            navigate_workspace_down: BindingConfig::one("down"),
            navigate_pane_left: BindingConfig::one("h"),
            navigate_pane_down: BindingConfig::one("j"),
            navigate_pane_up: BindingConfig::one("k"),
            navigate_pane_right: BindingConfig::one("l"),
            detach: BindingConfig::one("prefix+q"),
            reload_config: BindingConfig::one("prefix+shift+r"),
            open_notification_target: BindingConfig::one("prefix+o"),
            previous_workspace: BindingConfig::empty(),
            next_workspace: BindingConfig::empty(),
            previous_agent: BindingConfig::empty(),
            next_agent: BindingConfig::empty(),
            focus_agent: BindingConfig::empty(),
            remote_image_paste: "ctrl+v".into(),
            new_tab: BindingConfig::one("prefix+c"),
            rename_tab: BindingConfig::one("prefix+shift+t"),
            previous_tab: BindingConfig::one("prefix+p"),
            next_tab: BindingConfig::one("prefix+n"),
            move_tab_previous: BindingConfig::empty(),
            move_tab_next: BindingConfig::empty(),
            switch_tab: BindingConfig::one("prefix+1..9"),
            switch_workspace: BindingConfig::empty(),
            close_tab: BindingConfig::one("prefix+shift+x"),
            rename_pane: BindingConfig::one("prefix+shift+p"),
            edit_scrollback: BindingConfig::one("prefix+e"),
            clear_pane: BindingConfig::default(),
            copy_mode: BindingConfig::one("prefix+["),
            focus_pane_left: BindingConfig::one("prefix+h"),
            focus_pane_down: BindingConfig::one("prefix+j"),
            focus_pane_up: BindingConfig::one("prefix+k"),
            focus_pane_right: BindingConfig::one("prefix+l"),
            swap_pane_left: BindingConfig::one("prefix+shift+h"),
            swap_pane_down: BindingConfig::one("prefix+shift+j"),
            swap_pane_up: BindingConfig::one("prefix+shift+k"),
            swap_pane_right: BindingConfig::one("prefix+shift+l"),
            cycle_pane_next: BindingConfig::one("prefix+tab"),
            cycle_pane_previous: BindingConfig::one("prefix+shift+tab"),
            last_pane: BindingConfig::empty(),
            split_vertical: BindingConfig::one("prefix+v"),
            split_horizontal: BindingConfig::one("prefix+minus"),
            close_pane: BindingConfig::one("prefix+x"),
            zoom: BindingConfig::one("prefix+z"),
            resize_mode: BindingConfig::one("prefix+r"),
            resize_pane_left: BindingConfig::empty(),
            resize_pane_down: BindingConfig::empty(),
            resize_pane_up: BindingConfig::empty(),
            resize_pane_right: BindingConfig::empty(),
            toggle_sidebar: BindingConfig::one("prefix+b"),
            indexed: IndexedKeysConfig::default(),
            command: Vec::new(),
        }
    }
}

impl KeysConfig {
    /// Whether the user file set `field` — decides if its parsed bindings
    /// register as User (displacing colliding defaults) in the registry.
    pub(crate) fn key_field_is_user_configured(&self, field: &str) -> bool {
        self.user_fields.contains(field)
    }
}

impl<'de> Deserialize<'de> for KeysConfig {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let input = KeysConfigOverlay::deserialize(deserializer)?;
        let mut keys = KeysConfig::default();

        let prefix_was_supplied = input.prefix.is_some() || input.extra_prefixes.is_some();
        let mut prefix_values = Vec::new();
        if let Some(prefix) = input.prefix {
            prefix_values.extend(prefix.into_values());
        }
        if let Some(extra) = input.extra_prefixes {
            prefix_values.extend(extra.into_values());
        }
        if prefix_was_supplied {
            // An explicitly empty list stays empty so prefix validation
            // rejects it, matching the reference merge.
            keys.prefix = match prefix_values.len() {
                0 => BindingConfig::Many(Vec::new()),
                1 => BindingConfig::One(prefix_values.remove(0)),
                _ => BindingConfig::Many(prefix_values),
            };
            keys.user_fields.insert("prefix");
        }

        macro_rules! apply_field {
            ($field:ident) => {
                if let Some(value) = input.$field {
                    keys.$field = value;
                    keys.user_fields.insert(stringify!($field));
                }
            };
        }

        apply_field!(help);
        apply_field!(settings);
        apply_field!(new_workspace);
        apply_field!(new_worktree);
        apply_field!(open_worktree);
        apply_field!(remove_worktree);
        apply_field!(rename_workspace);
        apply_field!(close_workspace);
        apply_field!(workspace_picker);
        apply_field!(goto);
        apply_field!(navigate_workspace_up);
        apply_field!(navigate_workspace_down);
        apply_field!(navigate_pane_left);
        apply_field!(navigate_pane_down);
        apply_field!(navigate_pane_up);
        apply_field!(navigate_pane_right);
        apply_field!(detach);
        apply_field!(reload_config);
        apply_field!(open_notification_target);
        apply_field!(previous_workspace);
        apply_field!(next_workspace);
        apply_field!(previous_agent);
        apply_field!(next_agent);
        apply_field!(focus_agent);
        apply_field!(remote_image_paste);
        apply_field!(new_tab);
        apply_field!(rename_tab);
        apply_field!(previous_tab);
        apply_field!(next_tab);
        apply_field!(move_tab_previous);
        apply_field!(move_tab_next);
        apply_field!(switch_tab);
        apply_field!(switch_workspace);
        apply_field!(close_tab);
        apply_field!(rename_pane);
        apply_field!(edit_scrollback);
        apply_field!(clear_pane);
        apply_field!(copy_mode);
        apply_field!(focus_pane_left);
        apply_field!(focus_pane_down);
        apply_field!(focus_pane_up);
        apply_field!(focus_pane_right);
        apply_field!(swap_pane_left);
        apply_field!(swap_pane_down);
        apply_field!(swap_pane_up);
        apply_field!(swap_pane_right);
        apply_field!(cycle_pane_next);
        apply_field!(cycle_pane_previous);
        apply_field!(last_pane);
        apply_field!(split_vertical);
        apply_field!(split_horizontal);
        apply_field!(close_pane);
        apply_field!(zoom);
        apply_field!(resize_mode);
        apply_field!(resize_pane_left);
        apply_field!(resize_pane_down);
        apply_field!(resize_pane_up);
        apply_field!(resize_pane_right);
        apply_field!(toggle_sidebar);
        apply_field!(indexed);
        apply_field!(command);

        Ok(keys)
    }
}
