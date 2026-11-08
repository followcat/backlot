// Derived from herdr src/config/io.rs (Apache-2.0).
//
//! Read-only config loading: BOM normalization, `serde_ignored` unused-key
//! tracking, unknown-section/key diagnostics, and the diagnostic summary —
//! exactly the reference `Config::load` flow for the sections hc-tui
//! consumes. The user config is never written or migrated.

use std::path::Path;

use super::Config;

mod live;
pub use live::load_live;

#[cfg(test)]
#[path = "io_tests.rs"]
mod tests;

/// Top-level sections the reference config model recognizes. Sections
/// hc-tui does not consume still count as known — they are ignored
/// silently like the reference ignores sections this client skips, never
/// flagged as unknown.
const KNOWN_TOP_LEVEL_CONFIG_KEYS: &[&str] = &[
    "advanced",
    "experimental",
    "keys",
    "onboarding",
    "remote",
    "server",
    "session",
    "terminal",
    "theme",
    "ui",
    "update",
    "worktrees",
];

/// Top-level sections hc-tui actually models. A `serde_ignored` path under
/// one of these means a real typo worth diagnosing; a path under a known
/// section this client does not consume (`terminal`, `server`, `remote`,
/// `session`, `advanced`) is a scoped deferral, not an unknown key.
const CONSUMED_TOP_LEVEL_CONFIG_KEYS: &[&str] = &[
    "onboarding",
    "theme",
    "update",
    "keys",
    "ui",
    "worktrees",
    "experimental",
];

#[derive(Debug)]
pub struct LoadedConfig {
    pub config: Config,
    pub diagnostics: Vec<String>,
    /// Sections that parsed but failed validation (live reload only —
    /// always empty for the initial read).
    pub invalid_sections: Vec<String>,
}

/// Ported `normalize_utf8_bom`: drops a leading BOM plus any line-start
/// BOM a line-oriented edit displaced into the middle of the file — only
/// BOMs the TOML parser actually rejects. A U+FEFF that is valid string
/// data is kept, because its parse error would not point at it.
fn normalize_utf8_bom(content: &str) -> String {
    let content = content.strip_prefix('\u{feff}').unwrap_or(content);
    if !content.contains('\u{feff}') {
        return content.to_owned();
    }

    let mut normalized = content.to_owned();
    while let Err(error) = toml::from_str::<toml::Value>(&normalized) {
        let Some(span) = error.span() else {
            break;
        };
        if normalized.get(span.clone()) != Some("\u{feff}") {
            break;
        }
        normalized.replace_range(span, "");
    }
    normalized
}

pub(crate) fn read_optional_config(path: &Path) -> std::io::Result<Option<String>> {
    match std::fs::read_to_string(path) {
        Ok(content) => Ok(Some(normalize_utf8_bom(&content))),
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(err) => Err(err),
    }
}

/// Read-only config load for `path`: missing/unreadable files yield
/// defaults plus a diagnostic, exactly the reference fallback.
pub fn load(path: &Path) -> LoadedConfig {
    let content = match read_optional_config(path) {
        Ok(Some(content)) => content,
        Ok(None) => {
            return LoadedConfig {
                config: Config::default(),
                diagnostics: Vec::new(),
                invalid_sections: Vec::new(),
            };
        }
        Err(err) => {
            return LoadedConfig {
                config: Config::default(),
                diagnostics: vec![format!("config read error: {err}; using defaults")],
                invalid_sections: Vec::new(),
            };
        }
    };

    let deserializer = toml::Deserializer::new(&content);
    match deserialize_with_ignored::<Config, _>(deserializer) {
        Ok((config, ignored_keys)) => {
            let (unknown_sections, mut diagnostics) = unknown_top_level_sections_from_str(&content);
            diagnostics.extend(unknown_config_key_diagnostics(
                ignored_keys
                    .into_iter()
                    .filter(|path| {
                        // Ignore only known sections this client defers.
                        // Unknown scalar/empty-array roots still need key
                        // diagnostics; unknown tables have section diagnostics.
                        !matches!(path.first(), Some(ConfigKeyPathSegment::Key(key))
                            if KNOWN_TOP_LEVEL_CONFIG_KEYS.contains(&key.as_str())
                                && !CONSUMED_TOP_LEVEL_CONFIG_KEYS.contains(&key.as_str()))
                            && !matches!(path.as_slice(), [ConfigKeyPathSegment::Key(key)] if unknown_sections.contains(key))
                    })
                    .collect(),
                None,
            ));
            diagnostics.extend(config.collect_diagnostics());
            LoadedConfig {
                config,
                diagnostics,
                invalid_sections: Vec::new(),
            }
        }
        Err(err) => LoadedConfig {
            config: Config::default(),
            diagnostics: vec![format!("config parse error: {err}; using defaults")],
            invalid_sections: Vec::new(),
        },
    }
}

/// The reference `config_diagnostic_summary`: names the file and an
/// impact suffix derived from the diagnostic kinds.
pub fn config_diagnostic_summary(diagnostics: &[String]) -> Option<String> {
    if diagnostics.is_empty() {
        return None;
    }

    let target = super::config_path()
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("config.toml")
        .to_string();
    let read_error = diagnostics
        .iter()
        .any(|diagnostic| diagnostic.starts_with("config read error:"));
    let impact = if diagnostics
        .iter()
        .any(|diagnostic| diagnostic.contains("using defaults"))
    {
        if read_error {
            " unreadable; using defaults"
        } else {
            " invalid; using defaults"
        }
    } else if diagnostics
        .iter()
        .any(|diagnostic| diagnostic.contains("keeping current config"))
    {
        if read_error {
            " unreadable; keeping current config"
        } else {
            " invalid; keeping current config"
        }
    } else if diagnostics
        .iter()
        .all(|diagnostic| diagnostic.starts_with("unknown config key "))
    {
        " has unknown keys"
    } else {
        ""
    };

    Some(format!("{target}{impact}; herdr config check"))
}

fn unknown_top_level_sections_from_str(content: &str) -> (Vec<String>, Vec<String>) {
    let Ok(value) = toml::from_str::<toml::Value>(content) else {
        return (Vec::new(), Vec::new());
    };
    let Some(table) = value.as_table() else {
        return (Vec::new(), Vec::new());
    };

    let mut keys = Vec::new();
    let mut diagnostics = Vec::new();
    for (key, value) in table {
        if let Some(diagnostic) = unknown_top_level_section_diagnostic(key, value) {
            keys.push(key.clone());
            diagnostics.push(diagnostic);
        }
    }
    (keys, diagnostics)
}

fn unknown_top_level_section_diagnostic(key: &str, value: &toml::Value) -> Option<String> {
    if KNOWN_TOP_LEVEL_CONFIG_KEYS.contains(&key) {
        return None;
    }

    let header = if value.is_table() {
        format!("[{key}]")
    } else if value
        .as_array()
        .is_some_and(|items| !items.is_empty() && items.iter().all(toml::Value::is_table))
    {
        format!("[[{key}]]")
    } else {
        return None;
    };

    if key == "toast" {
        Some(format!(
            "unknown config section {header}; did you mean [ui.toast]? ignoring section"
        ))
    } else {
        Some(format!("unknown config section {header}; ignoring section"))
    }
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
enum ConfigKeyPathSegment {
    Key(String),
    Index(usize),
}

fn unknown_config_key_diagnostics(
    paths: Vec<Vec<ConfigKeyPathSegment>>,
    section: Option<&str>,
) -> Vec<String> {
    let mut paths: Vec<Vec<ConfigKeyPathSegment>> = paths
        .into_iter()
        .map(|mut path| {
            if let Some(section) = section {
                path.insert(0, ConfigKeyPathSegment::Key(section.to_string()));
            }
            path
        })
        .collect();
    paths.sort();
    paths.dedup();
    paths
        .into_iter()
        .map(|path| {
            format!(
                "unknown config key {}; ignoring key",
                format_config_key_path(&path)
            )
        })
        .collect()
}

fn config_key_path(path: &serde_ignored::Path<'_>) -> Vec<ConfigKeyPathSegment> {
    fn visit(path: &serde_ignored::Path<'_>, segments: &mut Vec<ConfigKeyPathSegment>) {
        match path {
            serde_ignored::Path::Root => {}
            serde_ignored::Path::Seq { parent, index } => {
                visit(parent, segments);
                segments.push(ConfigKeyPathSegment::Index(*index));
            }
            serde_ignored::Path::Map { parent, key } => {
                visit(parent, segments);
                segments.push(ConfigKeyPathSegment::Key(key.clone()));
            }
            serde_ignored::Path::Some { parent }
            | serde_ignored::Path::NewtypeStruct { parent }
            | serde_ignored::Path::NewtypeVariant { parent } => visit(parent, segments),
        }
    }

    let mut segments = Vec::new();
    visit(path, &mut segments);
    segments
}

fn format_config_key_path(path: &[ConfigKeyPathSegment]) -> String {
    path.iter()
        .map(|segment| match segment {
            ConfigKeyPathSegment::Key(key)
                if !key.is_empty()
                    && key.bytes().all(|byte| {
                        byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-')
                    }) =>
            {
                key.clone()
            }
            ConfigKeyPathSegment::Key(key) => format!("{key:?}"),
            ConfigKeyPathSegment::Index(index) => index.to_string(),
        })
        .collect::<Vec<_>>()
        .join(".")
}

fn deserialize_with_ignored<'de, T, D>(
    deserializer: D,
) -> Result<(T, Vec<Vec<ConfigKeyPathSegment>>), D::Error>
where
    T: serde::Deserialize<'de>,
    D: serde::Deserializer<'de>,
{
    let mut ignored = Vec::new();
    let value = serde_ignored::deserialize(deserializer, |path| {
        ignored.push(config_key_path(&path));
    })?;
    Ok((value, ignored))
}
