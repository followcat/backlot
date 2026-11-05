// Derived from herdr src/config/io.rs (Apache-2.0).
//! Live loading retains invalid sections instead of replacing them with defaults.
use super::*;

pub fn load_live(path: &Path) -> Result<LoadedConfig, Vec<String>> {
    let content = match read_optional_config(path) {
        Ok(Some(content)) => content,
        Ok(None) => {
            return Ok(LoadedConfig {
                config: Config::default(),
                diagnostics: Vec::new(),
                invalid_sections: Vec::new(),
            });
        }
        Err(error) => {
            return Err(vec![format!(
                "config read error: {error}; keeping current config"
            )]);
        }
    };
    let value = content.parse::<toml::Value>().map_err(|error| {
        vec![format!(
            "config parse error: {error}; keeping current config"
        )]
    })?;
    let table = value.as_table().ok_or_else(|| {
        vec!["config parse error: top-level config must be a table; keeping current config".into()]
    })?;
    let mut config = Config::default();
    let (unknown_sections, mut diagnostics) = unknown_top_level_sections_from_str(&content);
    diagnostics.extend(unknown_config_key_diagnostics(
        table
            .keys()
            .filter(|key| {
                !KNOWN_TOP_LEVEL_CONFIG_KEYS.contains(&key.as_str())
                    && !unknown_sections.contains(key)
            })
            .map(|key| vec![ConfigKeyPathSegment::Key(key.clone())])
            .collect(),
        None,
    ));
    let mut invalid_sections = Vec::new();
    macro_rules! section {
        ($field:ident, $label:literal) => {
            load_section(
                table,
                stringify!($field),
                $label,
                &mut diagnostics,
                &mut invalid_sections,
                |section| config.$field = section,
            );
        };
    }
    section!(theme, "theme config");
    section!(keys, "keybinding config");
    section!(ui, "ui config");
    section!(worktrees, "worktree config");
    section!(experimental, "experimental config");
    section!(update, "update config");
    diagnostics.extend(config.theme.diagnostics());
    Ok(LoadedConfig {
        config,
        diagnostics,
        invalid_sections,
    })
}

fn load_section<T: serde::de::DeserializeOwned>(
    table: &toml::map::Map<String, toml::Value>,
    section: &str,
    label: &str,
    diagnostics: &mut Vec<String>,
    invalid: &mut Vec<String>,
    apply: impl FnOnce(T),
) {
    let Some(value) = table.get(section) else {
        return;
    };
    match deserialize_with_ignored(value.clone()) {
        Ok((config, ignored)) => {
            diagnostics.extend(unknown_config_key_diagnostics(ignored, Some(section)));
            apply(config);
        }
        Err(error) => {
            diagnostics.push(format!(
                "invalid {label}: {error}; keeping current {section} settings"
            ));
            invalid.push(section.into());
        }
    }
}
