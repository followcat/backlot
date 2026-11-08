// Derived from herdr src/config/keybinds.rs (Apache-2.0).
//
//! The binding registry behind `validated_keybinds`: user bindings register
//! before defaults so a user binding silently displaces a colliding default,
//! prefix keys are reserved as direct keys, navigate keys live in their own
//! registry, and unsafe/unbindable entries emit the reference diagnostics.

use crossterm::event::{KeyCode, KeyModifiers};

use super::keybinds::{
    ActionKeybinds, BindingConfig, BindingTrigger, CustomCommandAction, CustomCommandKeybind,
    IndexedKeybind, KeyCombo, Keybinds, NavigateKeybinds, ParsedBinding, ResolvedBinding,
    is_unmodified_printable, normalize_key_combo, parse_binding_string, parse_key_combo,
    parse_prefix_keys,
};
use super::{CommandKeybindType, Config};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum BindingSource {
    User,
    Default,
}

struct RegisteredBinding {
    field: String,
    source: BindingSource,
}

pub(crate) struct BindingRegistry {
    prefix_combos: Vec<KeyCombo>,
    prefix_source: BindingSource,
    direct: std::collections::HashMap<KeyCombo, RegisteredBinding>,
    prefix: std::collections::HashMap<KeyCombo, RegisteredBinding>,
}

impl BindingRegistry {
    fn new(prefix_combos: Vec<KeyCombo>, prefix_source: BindingSource) -> Self {
        let mut normalized: Vec<KeyCombo> =
            prefix_combos.into_iter().map(normalize_key_combo).collect();
        normalized.dedup();
        Self {
            prefix_combos: normalized,
            prefix_source,
            direct: std::collections::HashMap::new(),
            prefix: std::collections::HashMap::new(),
        }
    }

    fn reserve_direct(&mut self, combo: KeyCombo, field: &str, source: BindingSource) {
        self.direct
            .entry(normalize_key_combo(combo))
            .or_insert_with(|| RegisteredBinding {
                field: field.to_string(),
                source,
            });
    }

    /// Reserve every configured prefix key as a direct/modifier key so no
    /// action can hijack it before prefix mode starts.
    fn reserve_prefix_keys(&mut self, field: &str, source: BindingSource) {
        let combos = self.prefix_combos.clone();
        for combo in combos {
            self.reserve_direct(combo, field, source);
        }
    }

    fn prefix_rhs_is_reserved(&self, combo: KeyCombo) -> bool {
        let combo = normalize_key_combo(combo);
        self.prefix_combos.contains(&combo)
    }

    fn conflict(&self, binding: &ResolvedBinding) -> Option<&RegisteredBinding> {
        match binding.trigger {
            BindingTrigger::Direct(combo) => self.direct.get(&normalize_key_combo(combo)),
            BindingTrigger::Prefix(combo) => self.prefix.get(&normalize_key_combo(combo)),
        }
    }

    fn register(&mut self, binding: &ResolvedBinding, field: &str, source: BindingSource) {
        let registered = || RegisteredBinding {
            field: field.to_string(),
            source,
        };
        match binding.trigger {
            BindingTrigger::Direct(combo) => {
                self.direct.insert(normalize_key_combo(combo), registered());
            }
            BindingTrigger::Prefix(combo) => {
                self.prefix.insert(normalize_key_combo(combo), registered());
            }
        }
    }
}

fn reserve_navigate_runtime_keys(registry: &mut BindingRegistry) {
    for combo in [
        (KeyCode::Esc, KeyModifiers::empty()),
        (KeyCode::Enter, KeyModifiers::empty()),
        (KeyCode::Tab, KeyModifiers::empty()),
        (KeyCode::BackTab, KeyModifiers::empty()),
        (KeyCode::Tab, KeyModifiers::SHIFT),
        (KeyCode::Left, KeyModifiers::empty()),
        (KeyCode::Right, KeyModifiers::empty()),
    ] {
        registry.reserve_direct(combo, "navigate reserved keys", BindingSource::Default);
    }

    for idx in '1'..='9' {
        registry.reserve_direct(
            (KeyCode::Char(idx), KeyModifiers::empty()),
            "navigate reserved keys",
            BindingSource::Default,
        );
    }
}

fn parse_action_bindings(
    field: &str,
    config: &BindingConfig,
    registry: &mut BindingRegistry,
    diagnostics: &mut Vec<String>,
    source: BindingSource,
) -> ActionKeybinds {
    let mut bindings = Vec::new();
    for raw in config.values() {
        let raw = raw.trim();
        if raw.is_empty() {
            continue;
        }
        match parse_binding_string(raw) {
            Some(ParsedBinding::Single(binding)) => {
                if reject_binding(field, &binding, registry, diagnostics, source) {
                    continue;
                }
                registry.register(&binding, field, source);
                bindings.push(binding);
            }
            Some(ParsedBinding::Range(_)) => {
                diagnostics.push(format!(
                    "range keybinding is only valid for indexed actions: {field} = {raw:?}; disabling binding"
                ));
            }
            None => {
                diagnostics.push(format!(
                    "invalid keybinding: {field} = {raw:?}; disabling binding"
                ));
            }
        }
    }
    ActionKeybinds { bindings }
}

fn parse_navigate_bindings(
    field: &'static str,
    config: &BindingConfig,
    registry: &mut BindingRegistry,
    diagnostics: &mut Vec<String>,
    source: BindingSource,
) -> ActionKeybinds {
    let mut bindings = Vec::new();
    for raw in config.values() {
        let raw = raw.trim();
        if raw.is_empty() {
            continue;
        }
        match parse_binding_string(raw) {
            Some(ParsedBinding::Single(binding)) => {
                if reject_navigate_binding(field, &binding, registry, diagnostics, source) {
                    continue;
                }
                registry.register(&binding, field, source);
                bindings.push(binding);
            }
            Some(ParsedBinding::Range(_)) => {
                diagnostics.push(format!(
                    "range keybinding is only valid for indexed actions: {field} = {raw:?}; disabling binding"
                ));
            }
            None => {
                diagnostics.push(format!(
                    "invalid keybinding: {field} = {raw:?}; disabling binding"
                ));
            }
        }
    }
    ActionKeybinds { bindings }
}

fn parse_indexed_bindings(
    field: &'static str,
    config: &BindingConfig,
    registry: &mut BindingRegistry,
    diagnostics: &mut Vec<String>,
    source: BindingSource,
) -> Vec<IndexedKeybind> {
    let mut bindings = Vec::new();
    for raw in config.values() {
        let raw = raw.trim();
        if raw.is_empty() {
            continue;
        }
        match parse_binding_string(raw) {
            Some(ParsedBinding::Single(binding)) => {
                push_indexed_binding(field, binding, registry, diagnostics, source, &mut bindings);
            }
            Some(ParsedBinding::Range(range)) => {
                for binding in range {
                    push_indexed_binding(
                        field,
                        binding,
                        registry,
                        diagnostics,
                        source,
                        &mut bindings,
                    );
                }
            }
            None => {
                diagnostics.push(format!(
                    "invalid keybinding: {field} = {raw:?}; disabling binding"
                ));
            }
        }
    }
    bindings
}

fn push_indexed_binding(
    field: &str,
    binding: ResolvedBinding,
    registry: &mut BindingRegistry,
    diagnostics: &mut Vec<String>,
    source: BindingSource,
    bindings: &mut Vec<IndexedKeybind>,
) {
    if !matches!(binding.trigger.combo().0, KeyCode::Char('1'..='9')) {
        diagnostics.push(format!(
            "indexed keybinding must use 1..9: {field} = {:?}; disabling binding",
            binding.label
        ));
        return;
    }
    if reject_binding(field, &binding, registry, diagnostics, source) {
        return;
    }
    registry.register(&binding, field, source);
    bindings.push(IndexedKeybind {
        trigger: binding.trigger,
        label: binding.label,
    });
}

fn append_legacy_indexed_bindings(
    target: &mut Vec<IndexedKeybind>,
    field: &'static str,
    configured_label: &str,
    registry: &mut BindingRegistry,
    diagnostics: &mut Vec<String>,
    source: BindingSource,
) {
    if configured_label.trim().is_empty() {
        return;
    }
    let Some(modifiers) = parse_modifier_combo(configured_label) else {
        diagnostics.push(format!(
            "invalid indexed keybinding: {field} = {configured_label:?}; disabling binding"
        ));
        return;
    };

    for idx in 1..=9 {
        let combo = (
            KeyCode::Char(char::from_digit(idx, 10).unwrap_or('1')),
            modifiers,
        );
        let binding = ResolvedBinding {
            trigger: BindingTrigger::Direct(combo),
            label: format!("{}+{idx}", configured_label.trim()),
        };
        if reject_binding(field, &binding, registry, diagnostics, source) {
            continue;
        }
        registry.register(&binding, field, source);
        target.push(IndexedKeybind {
            trigger: binding.trigger,
            label: binding.label,
        });
    }
}

fn parse_modifier_combo(raw: &str) -> Option<KeyModifiers> {
    parse_key_combo(raw.trim()).map(|(_, modifiers)| modifiers)
}

fn reject_navigate_binding(
    field: &str,
    binding: &ResolvedBinding,
    registry: &BindingRegistry,
    diagnostics: &mut Vec<String>,
    source: BindingSource,
) -> bool {
    if binding.trigger.is_prefix() {
        diagnostics.push(format!(
            "navigate keybinding must not include prefix: {field} = {:?}; disabling binding",
            binding.label
        ));
        return true;
    }

    if matches!(normalize_key_combo(binding.trigger.combo()).0, KeyCode::Esc) {
        diagnostics.push(format!(
            "navigate keybinding cannot use esc: {field} = {:?}; disabling binding",
            binding.label
        ));
        return true;
    }

    if let Some(first_binding) = registry.conflict(binding) {
        if source == BindingSource::Default && first_binding.source == BindingSource::User {
            return true;
        }
        let first_field = &first_binding.field;
        diagnostics.push(format!(
            "{}: kept {first_field}, disabled {field}",
            binding.label
        ));
        return true;
    }

    false
}

fn reject_binding(
    field: &str,
    binding: &ResolvedBinding,
    registry: &BindingRegistry,
    diagnostics: &mut Vec<String>,
    source: BindingSource,
) -> bool {
    if binding.trigger.is_prefix() && registry.prefix_rhs_is_reserved(binding.trigger.combo()) {
        if source == BindingSource::Default && registry.prefix_source == BindingSource::User {
            return true;
        }
        diagnostics.push(format!(
            "reserved keybinding: {field} = {:?} uses keys.prefix as the prefix-mode key; pressing the prefix twice sends a literal prefix key, so this binding is disabled",
            binding.label
        ));
        return true;
    }

    if let Some(first_binding) = registry.conflict(binding) {
        if source == BindingSource::Default && first_binding.source == BindingSource::User {
            return true;
        }
        let first_field = &first_binding.field;
        diagnostics.push(format!(
            "{}: kept {first_field}, disabled {field}",
            binding.label
        ));
        return true;
    }

    if binding.trigger.is_direct() && is_unmodified_printable(binding.trigger.combo()) {
        let suggestion = format!("prefix+{}", binding.label);
        diagnostics.push(format!(
            "unsafe direct keybinding: {field} = {:?} would intercept typing; use {:?} to require the prefix; disabling binding",
            binding.label, suggestion
        ));
        return true;
    }

    false
}

fn append_custom_command_bindings(
    config: &Config,
    keybinds: &mut Keybinds,
    registry: &mut BindingRegistry,
    diagnostics: &mut Vec<String>,
) {
    for (index, command) in config.keys.command.iter().enumerate() {
        let key_field = format!("keys.command[{index}].key");
        let command_field = format!("keys.command[{index}].command");

        if command.command.trim().is_empty() {
            diagnostics.push(format!(
                "empty custom command: {command_field}; disabling custom command"
            ));
            continue;
        }

        let bindings = parse_action_bindings(
            &key_field,
            &command.key,
            registry,
            diagnostics,
            BindingSource::User,
        );
        if bindings.bindings.is_empty() {
            continue;
        }

        let action = match command.action_type {
            CommandKeybindType::Shell => CustomCommandAction::Shell,
            CommandKeybindType::Pane => CustomCommandAction::Pane,
            CommandKeybindType::Popup => CustomCommandAction::Popup,
            CommandKeybindType::PluginAction => CustomCommandAction::PluginAction,
        };
        let mut width = None;
        let mut height = None;
        if action != CustomCommandAction::Popup
            && (command.width.is_some() || command.height.is_some())
        {
            diagnostics.push(format!(
                "popup size on non-popup custom command: keys.command[{index}]; ignoring width and height"
            ));
        } else {
            for (slot, raw) in [(&mut width, &command.width), (&mut height, &command.height)] {
                match raw.as_ref().map(crate::popup_size::PopupSize::from_toml) {
                    None => {}
                    Some(Ok(size)) => *slot = Some(size),
                    Some(Err(error)) => diagnostics.push(format!(
                        "invalid popup size on keys.command[{index}]: {error}"
                    )),
                }
            }
        }
        let label = bindings.label().unwrap_or_else(|| "unset".to_string());
        keybinds.custom_commands.push(CustomCommandKeybind {
            bindings,
            label,
            command: command.command.clone(),
            action,
            description: command.description.clone(),
            width,
            height,
        });
    }
}

#[path = "registry_resolve.rs"]
mod resolve;
