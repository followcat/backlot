//! `<state base>/config.toml` parsing: `[runner_env.<runner>]` string
//! maps hold per-runner settings a role terminal gets through
//! `terminal.run`'s env — a proxy for Devin, say. Values never reach
//! labels, the ledger, the summary or logs.
//!
//! Also the `[hollywood]` section of the user config
//! (`$XDG_CONFIG_HOME/backlot/config.toml`, default
//! `~/.config/backlot/config.toml`): the overseer's runner settings
//! (D14). Nothing launches from it in H1 — `status --all` shows it.

use std::collections::BTreeMap;
use std::path::PathBuf;

/// `line L column C` for a byte offset — the only part of a parse
/// error that may be reported: the parser's message quotes the
/// offending line, and a config file may hold secrets (a proxy URL
/// with credentials, say) that must never reach the ledger.
pub(crate) fn line_col(text: &str, pos: usize) -> String {
    let upto = &text[..pos.min(text.len())];
    let line = upto.bytes().filter(|b| *b == b'\n').count() + 1;
    let col = upto
        .rsplit('\n')
        .next()
        .map_or(1, |l| l.chars().count() + 1);
    format!("line {line} column {col}")
}

/// The `[runner_env.<runner>]` map inside `text`. A missing section
/// is an empty map; malformed TOML or a non-string value is the
/// error detail a `config malformed` attention carries — position
/// only, never any of the file's text.
pub fn runner_env_in(text: &str, runner: &str) -> Result<BTreeMap<String, String>, String> {
    let parsed: toml::Value = toml::from_str(text).map_err(|e| match e.span() {
        Some(span) => format!("config.toml: malformed at {}", line_col(text, span.start)),
        None => "config.toml: malformed".to_string(),
    })?;
    let Some(section) = parsed.get("runner_env").and_then(|t| t.get(runner)) else {
        return Ok(BTreeMap::new());
    };
    let table = section
        .as_table()
        .ok_or_else(|| format!("config.toml: runner_env.{runner} is not a table"))?;
    let mut env = BTreeMap::new();
    for (k, v) in table {
        match v.as_str() {
            Some(s) => {
                env.insert(k.clone(), s.to_string());
            }
            None => {
                return Err(format!(
                    "config.toml: runner_env.{runner}.{k} is not a string"
                ));
            }
        }
    }
    Ok(env)
}

/// Hollywood's `[hollywood]` section: its runner identity, like a
/// role's `ActorIdentity` but with no studio. `key` is the layer's
/// toggle binding (D15a), `default_cast` the cast template a
/// `studio-new` draft starts from when the user says nothing (D19).
#[derive(Debug, Clone, PartialEq)]
pub struct HollywoodCfg {
    pub runner: String,
    pub model: String,
    pub effort: Option<String>,
    pub key: Option<String>,
    pub default_cast: Option<String>,
}

/// The user config's path: `$XDG_CONFIG_HOME/backlot/config.toml`,
/// `~/.config/backlot/config.toml` without it — the file the panel
/// already reads; `[hollywood]` sits beside its sections.
pub fn config_path(env: &BTreeMap<String, String>) -> PathBuf {
    let get = |name: &str| env.get(name).filter(|v| !v.is_empty());
    let root = match get("XDG_CONFIG_HOME") {
        Some(x) => PathBuf::from(x),
        None => PathBuf::from(get("HOME").map_or("/", String::as_str)).join(".config"),
    };
    root.join("backlot").join("config.toml")
}

/// The `[hollywood]` section inside `text`. A missing section is
/// `None` — the layer stays hidden (D15). A malformed file or a bad
/// value is the error, position only for the parse kind.
fn hollywood_in(text: &str) -> Result<Option<HollywoodCfg>, String> {
    let parsed: toml::Value = toml::from_str(text).map_err(|e| match e.span() {
        Some(span) => format!("config.toml: malformed at {}", line_col(text, span.start)),
        None => "config.toml: malformed".to_string(),
    })?;
    let Some(section) = parsed.get("hollywood") else {
        return Ok(None);
    };
    let table = section
        .as_table()
        .ok_or_else(|| "config.toml: hollywood is not a table".to_string())?;
    let string = |key: &str| -> Result<Option<String>, String> {
        match table.get(key) {
            None => Ok(None),
            Some(v) => match v.as_str() {
                Some(s) if !s.trim().is_empty() => Ok(Some(s.to_string())),
                _ => Err(format!(
                    "config.toml: hollywood.{key} must be a nonempty string"
                )),
            },
        }
    };
    // A table without `runner` carries client-only settings (`key`,
    // `start`, …) — the agent stays unconfigured; a `runner` without
    // `model` is still the malformed case.
    let Some(runner) = string("runner")? else {
        return Ok(None);
    };
    if !crate::runners::supported(&runner) {
        return Err(format!(
            "config.toml: hollywood.runner '{runner}' is not a supported runner"
        ));
    }
    let model =
        string("model")?.ok_or_else(|| "config.toml: hollywood.model is required".to_string())?;
    let effort = string("effort")?;
    effort_rule(&runner, effort.as_deref()).map_err(|e| format!("config.toml: {e}"))?;
    Ok(Some(HollywoodCfg {
        runner,
        model,
        effort,
        key: string("key")?,
        default_cast: string("default_cast")?,
    }))
}

/// The cast file's effort rule applied to Hollywood's own runner:
/// `cursor`, `devin`, `cmd` and `opencode` carry no effort flag;
/// every other runner needs one declared. `hollywood setup`
/// refuses on the same rule `hollywood_in` rejects with.
pub(crate) fn effort_rule(runner: &str, effort: Option<&str>) -> Result<(), String> {
    if matches!(runner, "cursor" | "cmd") && effort.is_some() {
        return Err(format!("hollywood.runner '{runner}' takes no effort"));
    }
    if !matches!(runner, "cursor" | "devin" | "cmd" | "opencode") && effort.is_none() {
        return Err(format!("hollywood.runner '{runner}' needs an effort"));
    }
    Ok(())
}

/// Read `[hollywood]` from the user config; a missing file or section
/// is `Ok(None)`.
pub fn hollywood(env: &BTreeMap<String, String>) -> Result<Option<HollywoodCfg>, String> {
    let path = config_path(env);
    match std::fs::read_to_string(&path) {
        Ok(text) => hollywood_in(&text),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(e) => Err(format!("config.toml: {e}")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A missing section or file-shaped nothing is no extra env.
    #[test]
    fn absent_runner_env_is_empty() {
        for text in ["", "# nothing\n", "[other]\nx = \"1\"\n"] {
            assert_eq!(runner_env_in(text, "devin").unwrap(), BTreeMap::new());
        }
        let text = "[runner_env.devin]\nPROXY = \"http://proxy:8080\"\n";
        assert_eq!(runner_env_in(text, "codex").unwrap(), BTreeMap::new());
    }

    /// String values pass through verbatim.
    #[test]
    fn runner_env_strings_pass_through() {
        let text = concat!(
            "[runner_env.devin]\n",
            "HTTPS_PROXY = \"http://proxy:8080\"\n",
            "TOKEN_FILE = \"/run/secrets/devin\"\n",
        );
        let env = runner_env_in(text, "devin").unwrap();
        assert_eq!(env["HTTPS_PROXY"], "http://proxy:8080");
        assert_eq!(env["TOKEN_FILE"], "/run/secrets/devin");
    }

    /// Malformed TOML and non-string values refuse.
    #[test]
    fn malformed_runner_env_is_an_error() {
        assert!(runner_env_in("[runner_env.devin\n", "devin").is_err());
        assert!(runner_env_in("[runner_env]\ndevin = 3\n", "devin").is_err());
        assert!(runner_env_in("[runner_env.devin]\nPORT = 8080\n", "devin").is_err());
    }

    /// The malformed detail is the position only: the parser's own
    /// message quotes the offending line, and a secret in it must
    /// not reach the ledger.
    #[test]
    fn malformed_detail_is_position_only() {
        let text = "[runner_env.devin]\nHTTPS_PROXY = http://user:pw@host\n";
        let err = runner_env_in(text, "devin").unwrap_err();
        assert!(err.contains("line 2"), "{err}");
        assert!(err.contains("column"), "{err}");
        assert!(!err.contains("pw"), "{err}");
        assert!(!err.contains("user"), "{err}");
        assert!(!err.contains("http"), "{err}");
    }

    /// No `[hollywood]` — the layer stays hidden (D15).
    #[test]
    fn absent_hollywood_is_none() {
        for text in ["", "# nothing\n", "[other]\nx = \"1\"\n"] {
            assert_eq!(hollywood_in(text).unwrap(), None);
        }
    }

    /// A full section parses — runner, model, effort and the two
    /// optional fields.
    #[test]
    fn a_full_hollywood_section_parses() {
        let text = concat!(
            "[hollywood]\n",
            "runner = \"fake\"\n",
            "model = \"model-h\"\n",
            "effort = \"high\"\n",
            "key = \"alt+y\"\n",
            "default_cast = \"/home/user/cast.toml\"\n",
        );
        let cfg = hollywood_in(text).unwrap().unwrap();
        assert_eq!(
            cfg,
            HollywoodCfg {
                runner: "fake".into(),
                model: "model-h".into(),
                effort: Some("high".into()),
                key: Some("alt+y".into()),
                default_cast: Some("/home/user/cast.toml".into()),
            }
        );
    }

    /// A table without `runner` is client-only — `key`, `start` and
    /// friends need no agent, so the read is `Ok(None)`. A `runner`
    /// without `model` is still malformed, and the runner must be
    /// one the stage has an adapter for.
    #[test]
    fn hollywood_requires_runner_and_model() {
        let cfg = hollywood_in("[hollywood]\nmodel = \"m\"\nkey = \"alt+h\"\n").unwrap();
        assert!(cfg.is_none(), "a runner-less table is client-only");
        let err = hollywood_in("[hollywood]\nrunner = \"fake\"\n").unwrap_err();
        assert!(err.contains("model"), "{err}");
        let err = hollywood_in("[hollywood]\nrunner = \"runner-x\"\nmodel = \"m\"\n").unwrap_err();
        assert!(err.contains("not a supported runner"), "{err}");
    }

    /// The cast file's effort rule holds here too: `cursor`, `devin`
    /// and `cmd` carry none; every other runner needs one.
    #[test]
    fn hollywood_effort_follows_the_cast_rule() {
        let err =
            hollywood_in("[hollywood]\nrunner = \"cursor\"\nmodel = \"m\"\neffort = \"high\"\n")
                .unwrap_err();
        assert!(err.contains("takes no effort"), "{err}");
        let err = hollywood_in("[hollywood]\nrunner = \"fake\"\nmodel = \"m\"\n").unwrap_err();
        assert!(err.contains("needs an effort"), "{err}");
        let cfg = hollywood_in("[hollywood]\nrunner = \"devin\"\nmodel = \"m\"\n")
            .unwrap()
            .unwrap();
        assert_eq!(cfg.effort, None);
    }

    /// A malformed `[hollywood]` file reports the position only —
    /// the section may sit beside secrets.
    #[test]
    fn a_bad_hollywood_file_reports_position_only() {
        let err = hollywood_in("[hollywood\n").unwrap_err();
        assert!(err.contains("line 1"), "{err}");
        let err = hollywood_in("[hollywood]\nrunner = 3\nmodel = \"m\"\n").unwrap_err();
        assert!(err.contains("nonempty string"), "{err}");
        assert!(!err.contains('3'), "{err}");
    }
}
