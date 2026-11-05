// Derived from herdr src/config/keybinds.rs (Apache-2.0).
use super::*;

impl Config {
    /// The reference `validated_keybinds`: user bindings register before
    /// defaults, so configured bindings silently displace colliding
    /// builtins while same-tier conflicts diagnose `kept X, disabled Y`.
    pub(in crate::config) fn validated_keybinds(
        &self,
    ) -> (Option<String>, Vec<KeyCombo>, Vec<String>, Keybinds) {
        let (prefix_keys, prefix_diag, mut diagnostics) = parse_prefix_keys(&self.keys.prefix);
        let prefix_source = if self.keys.key_field_is_user_configured("prefix") {
            BindingSource::User
        } else {
            BindingSource::Default
        };
        let mut registry = BindingRegistry::new(prefix_keys.clone(), prefix_source);
        registry.reserve_prefix_keys("keys.prefix", prefix_source);
        let mut navigate_registry = BindingRegistry::new(prefix_keys.clone(), prefix_source);
        navigate_registry.reserve_prefix_keys("keys.prefix", prefix_source);
        reserve_navigate_runtime_keys(&mut navigate_registry);
        let prefix = prefix_keys;

        let mut keybinds = Keybinds {
            navigate: NavigateKeybinds {
                workspace_up: ActionKeybinds::default(),
                workspace_down: ActionKeybinds::default(),
                pane_left: ActionKeybinds::default(),
                pane_down: ActionKeybinds::default(),
                pane_up: ActionKeybinds::default(),
                pane_right: ActionKeybinds::default(),
            },
            help: ActionKeybinds::default(),
            settings: ActionKeybinds::default(),
            new_workspace: ActionKeybinds::default(),
            new_worktree: ActionKeybinds::default(),
            open_worktree: ActionKeybinds::default(),
            remove_worktree: ActionKeybinds::default(),
            rename_workspace: ActionKeybinds::default(),
            close_workspace: ActionKeybinds::default(),
            workspace_picker: ActionKeybinds::default(),
            goto: ActionKeybinds::default(),
            detach: ActionKeybinds::default(),
            reload_config: ActionKeybinds::default(),
            open_notification_target: ActionKeybinds::default(),
            previous_workspace: ActionKeybinds::default(),
            next_workspace: ActionKeybinds::default(),
            previous_agent: ActionKeybinds::default(),
            next_agent: ActionKeybinds::default(),
            focus_agent: Vec::new(),
            new_tab: ActionKeybinds::default(),
            rename_tab: ActionKeybinds::default(),
            previous_tab: ActionKeybinds::default(),
            next_tab: ActionKeybinds::default(),
            move_tab_previous: ActionKeybinds::default(),
            move_tab_next: ActionKeybinds::default(),
            switch_tab: Vec::new(),
            switch_workspace: Vec::new(),
            close_tab: ActionKeybinds::default(),
            rename_pane: ActionKeybinds::default(),
            edit_scrollback: ActionKeybinds::default(),
            clear_pane: ActionKeybinds::default(),
            copy_mode: ActionKeybinds::default(),
            focus_pane_left: ActionKeybinds::default(),
            focus_pane_down: ActionKeybinds::default(),
            focus_pane_up: ActionKeybinds::default(),
            focus_pane_right: ActionKeybinds::default(),
            swap_pane_left: ActionKeybinds::default(),
            swap_pane_down: ActionKeybinds::default(),
            swap_pane_up: ActionKeybinds::default(),
            swap_pane_right: ActionKeybinds::default(),
            cycle_pane_next: ActionKeybinds::default(),
            cycle_pane_previous: ActionKeybinds::default(),
            last_pane: ActionKeybinds::default(),
            split_vertical: ActionKeybinds::default(),
            split_horizontal: ActionKeybinds::default(),
            close_pane: ActionKeybinds::default(),
            zoom: ActionKeybinds::default(),
            resize_mode: ActionKeybinds::default(),
            resize_pane_left: ActionKeybinds::default(),
            resize_pane_down: ActionKeybinds::default(),
            resize_pane_up: ActionKeybinds::default(),
            resize_pane_right: ActionKeybinds::default(),
            toggle_sidebar: ActionKeybinds::default(),
            custom_commands: Vec::new(),
        };

        macro_rules! field_source {
            ($field:ident) => {
                if self.keys.key_field_is_user_configured(stringify!($field)) {
                    BindingSource::User
                } else {
                    BindingSource::Default
                }
            };
        }
        macro_rules! apply_action {
            ($target:expr, $field:ident, $source:expr) => {
                if field_source!($field) == $source {
                    $target = parse_action_bindings(
                        concat!("keys.", stringify!($field)),
                        &self.keys.$field,
                        &mut registry,
                        &mut diagnostics,
                        $source,
                    );
                }
            };
        }
        macro_rules! apply_indexed {
            ($target:expr, $field:ident, $legacy_config:expr, $source:expr) => {
                if field_source!($field) == $source {
                    if $source == BindingSource::Default && !$legacy_config.trim().is_empty() {
                        // A legacy [keys.indexed] entry is user configuration for
                        // this target and should displace the modern default.
                    } else {
                        $target = parse_indexed_bindings(
                            concat!("keys.", stringify!($field)),
                            &self.keys.$field,
                            &mut registry,
                            &mut diagnostics,
                            $source,
                        );
                    }
                }
            };
        }
        macro_rules! apply_navigate {
            ($target:expr, $field:ident, $source:expr) => {
                if field_source!($field) == $source {
                    $target = parse_navigate_bindings(
                        concat!("keys.", stringify!($field)),
                        &self.keys.$field,
                        &mut navigate_registry,
                        &mut diagnostics,
                        $source,
                    );
                }
            };
        }

        for source in [BindingSource::User, BindingSource::Default] {
            apply_navigate!(
                keybinds.navigate.workspace_up,
                navigate_workspace_up,
                source
            );
            apply_navigate!(
                keybinds.navigate.workspace_down,
                navigate_workspace_down,
                source
            );
            apply_navigate!(keybinds.navigate.pane_left, navigate_pane_left, source);
            apply_navigate!(keybinds.navigate.pane_down, navigate_pane_down, source);
            apply_navigate!(keybinds.navigate.pane_up, navigate_pane_up, source);
            apply_navigate!(keybinds.navigate.pane_right, navigate_pane_right, source);
            apply_action!(keybinds.help, help, source);
            apply_action!(keybinds.settings, settings, source);
            apply_action!(keybinds.new_workspace, new_workspace, source);
            apply_action!(keybinds.new_worktree, new_worktree, source);
            apply_action!(keybinds.open_worktree, open_worktree, source);
            apply_action!(keybinds.remove_worktree, remove_worktree, source);
            apply_action!(keybinds.rename_workspace, rename_workspace, source);
            apply_action!(keybinds.close_workspace, close_workspace, source);
            apply_action!(keybinds.workspace_picker, workspace_picker, source);
            apply_action!(keybinds.goto, goto, source);
            apply_action!(keybinds.detach, detach, source);
            apply_action!(keybinds.reload_config, reload_config, source);
            apply_action!(
                keybinds.open_notification_target,
                open_notification_target,
                source
            );
            apply_action!(keybinds.previous_workspace, previous_workspace, source);
            apply_action!(keybinds.next_workspace, next_workspace, source);
            apply_action!(keybinds.previous_agent, previous_agent, source);
            apply_action!(keybinds.next_agent, next_agent, source);
            apply_indexed!(
                keybinds.focus_agent,
                focus_agent,
                &self.keys.indexed.agents,
                source
            );
            apply_action!(keybinds.new_tab, new_tab, source);
            apply_action!(keybinds.rename_tab, rename_tab, source);
            apply_action!(keybinds.previous_tab, previous_tab, source);
            apply_action!(keybinds.next_tab, next_tab, source);
            apply_action!(keybinds.move_tab_previous, move_tab_previous, source);
            apply_action!(keybinds.move_tab_next, move_tab_next, source);
            apply_indexed!(
                keybinds.switch_tab,
                switch_tab,
                &self.keys.indexed.tabs,
                source
            );
            apply_indexed!(
                keybinds.switch_workspace,
                switch_workspace,
                &self.keys.indexed.workspaces,
                source
            );
            apply_action!(keybinds.close_tab, close_tab, source);
            apply_action!(keybinds.rename_pane, rename_pane, source);
            apply_action!(keybinds.edit_scrollback, edit_scrollback, source);
            apply_action!(keybinds.clear_pane, clear_pane, source);
            apply_action!(keybinds.copy_mode, copy_mode, source);
            apply_action!(keybinds.focus_pane_left, focus_pane_left, source);
            apply_action!(keybinds.focus_pane_down, focus_pane_down, source);
            apply_action!(keybinds.focus_pane_up, focus_pane_up, source);
            apply_action!(keybinds.focus_pane_right, focus_pane_right, source);
            apply_action!(keybinds.swap_pane_left, swap_pane_left, source);
            apply_action!(keybinds.swap_pane_down, swap_pane_down, source);
            apply_action!(keybinds.swap_pane_up, swap_pane_up, source);
            apply_action!(keybinds.swap_pane_right, swap_pane_right, source);
            apply_action!(keybinds.last_pane, last_pane, source);
            apply_action!(keybinds.cycle_pane_next, cycle_pane_next, source);
            apply_action!(keybinds.cycle_pane_previous, cycle_pane_previous, source);
            apply_action!(keybinds.split_vertical, split_vertical, source);
            apply_action!(keybinds.split_horizontal, split_horizontal, source);
            apply_action!(keybinds.close_pane, close_pane, source);
            apply_action!(keybinds.zoom, zoom, source);
            apply_action!(keybinds.resize_mode, resize_mode, source);
            apply_action!(keybinds.resize_pane_left, resize_pane_left, source);
            apply_action!(keybinds.resize_pane_down, resize_pane_down, source);
            apply_action!(keybinds.resize_pane_up, resize_pane_up, source);
            apply_action!(keybinds.resize_pane_right, resize_pane_right, source);
            apply_action!(keybinds.toggle_sidebar, toggle_sidebar, source);

            if source == field_source!(indexed) {
                append_legacy_indexed_bindings(
                    &mut keybinds.switch_tab,
                    "keys.indexed.tabs",
                    &self.keys.indexed.tabs,
                    &mut registry,
                    &mut diagnostics,
                    source,
                );
                append_legacy_indexed_bindings(
                    &mut keybinds.switch_workspace,
                    "keys.indexed.workspaces",
                    &self.keys.indexed.workspaces,
                    &mut registry,
                    &mut diagnostics,
                    source,
                );
                append_legacy_indexed_bindings(
                    &mut keybinds.focus_agent,
                    "keys.indexed.agents",
                    &self.keys.indexed.agents,
                    &mut registry,
                    &mut diagnostics,
                    source,
                );
            }

            if source == BindingSource::User {
                append_custom_command_bindings(
                    self,
                    &mut keybinds,
                    &mut registry,
                    &mut diagnostics,
                );
            }
        }

        (prefix_diag, prefix, diagnostics, keybinds)
    }
}
