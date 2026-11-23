//! `<state base>/config.toml` parsing: `[runner_env.<runner>]` string
//! maps hold per-runner settings a role terminal gets through
//! `terminal.run`'s env — a proxy for Devin, say. `[env_file]` names
//! dotenv files read beneath them — `default` for every runner, a
//! runner's own key for one — and `<state base>/agent.env` applies
//! when it exists. Values never reach labels, the ledger, the
//! summary or logs.
//!
//! Also the `[hollywood]` section of the user config
//! (`$XDG_CONFIG_HOME/backlot/config.toml`, default
//! `~/.config/backlot/config.toml`): the overseer's runner settings
//! (D14). Nothing launches from it in H1 — `status --all` shows it.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

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

/// The `[env_file]` table inside `text`: `default` names a file
/// read for every runner, a runner's own key one read for that
/// runner only — the values are dotenv file paths, never env
/// values themselves. Absent is an empty map.
pub fn env_files_in(text: &str) -> Result<BTreeMap<String, String>, String> {
    let parsed: toml::Value = toml::from_str(text).map_err(|e| match e.span() {
        Some(span) => format!("config.toml: malformed at {}", line_col(text, span.start)),
        None => "config.toml: malformed".to_string(),
    })?;
    let Some(section) = parsed.get("env_file") else {
        return Ok(BTreeMap::new());
    };
    let table = section
        .as_table()
        .ok_or_else(|| "config.toml: env_file is not a table".to_string())?;
    let mut files = BTreeMap::new();
    for (k, v) in table {
        match v.as_str() {
            Some(s) => {
                files.insert(k.clone(), s.to_string());
            }
            None => {
                return Err(format!("config.toml: env_file.{k} is not a string"));
            }
        }
    }
    Ok(files)
}

/// A dotenv file's entries: `KEY=VALUE` lines — `KEY` trimmed, an
/// `export ` lead tolerated, `#` comments and blanks skipped, one
/// pair of matching surrounding quotes stripped. A line that does
/// not parse is the error, its number only — the file may hold
/// secrets that must never reach a log or the ledger.
pub fn env_file_map(text: &str) -> Result<BTreeMap<String, String>, String> {
    let mut env = BTreeMap::new();
    for (n, line) in text.lines().enumerate() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let line = line.strip_prefix("export ").unwrap_or(line).trim_start();
        let Some((k, v)) = line.split_once('=') else {
            return Err(format!("env file line {} is not KEY=VALUE", n + 1));
        };
        let k = k.trim();
        if k.is_empty() {
            return Err(format!("env file line {} is not KEY=VALUE", n + 1));
        }
        let v = v.trim();
        let v = if v.len() >= 2
            && ((v.starts_with('"') && v.ends_with('"'))
                || (v.starts_with('\'') && v.ends_with('\'')))
        {
            &v[1..v.len() - 1]
        } else {
            v
        };
        env.insert(k.to_string(), v.to_string());
    }
    Ok(env)
}

/// `~/` and `$HOME/` in a configured env-file path expand under
/// `home`; anything else is used as given.
fn env_file_path(p: &str, home: Option<&Path>) -> PathBuf {
    for prefix in ["~/", "$HOME/"] {
        if let Some(rest) = p.strip_prefix(prefix)
            && let Some(home) = home
        {
            return home.join(rest);
        }
    }
    PathBuf::from(p)
}

/// The env a `runner`'s launch gets, lowest layer first so a later
/// source wins: `<state_dir>/agent.env` when present — the
/// zero-config seam, beside the `config.toml` it complements —
/// then the `[env_file]` `default` and `runner` files, then the
/// `[runner_env.<runner>]` strings. A file that does not exist is
/// skipped; one that cannot be read or parsed is the error —
/// its path only, never its contents.
pub fn runner_env_for_home(
    state_dir: &Path,
    text: &str,
    runner: &str,
    home: &Path,
) -> Result<BTreeMap<String, String>, String> {
    runner_env_layered_home(state_dir, text, "", runner, home)
}

/// The layered launch env, lowest first: `<state_dir>/agent.env` and
/// the per-core `[env_file]` files, then the user config's
/// `[runner_env.<runner>]` (never its `[env_file]`), then the
/// per-core config's own `[runner_env.<runner>]` — which wins on any
/// key it sets. Both configs report a malformed file the same way:
/// the position only.
pub fn runner_env_layered_home(
    state_dir: &Path,
    core_text: &str,
    user_text: &str,
    runner: &str,
    home: &Path,
) -> Result<BTreeMap<String, String>, String> {
    let files = env_files_in(core_text)?;
    let mut paths = vec![state_dir.join("agent.env")];
    if let Some(d) = files.get("default") {
        paths.push(env_file_path(d, Some(home)));
    }
    if let Some(r) = files.get(runner) {
        paths.push(env_file_path(r, Some(home)));
    }
    let mut env = BTreeMap::new();
    for path in paths {
        match std::fs::read_to_string(&path) {
            Ok(t) => env.extend(env_file_map(&t).map_err(|e| format!("{}: {e}", path.display()))?),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => continue,
            Err(e) => return Err(format!("{}: {e}", path.display())),
        }
    }
    env.extend(runner_env_in(user_text, runner)?);
    env.extend(runner_env_in(core_text, runner)?);
    Ok(env)
}

/// [`runner_env_for_home`] with the process's own `HOME` — the
/// launch-time path.
pub fn runner_env_for(
    state_dir: &Path,
    text: &str,
    runner: &str,
) -> Result<BTreeMap<String, String>, String> {
    let home = std::env::var_os("HOME")
        .filter(|h| !h.is_empty())
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("/"));
    runner_env_for_home(state_dir, text, runner, &home)
}

/// [`runner_env_layered_home`] with the process's own `HOME` — the
/// stage's launch-time path.
pub fn runner_env_layered(
    state_dir: &Path,
    core_text: &str,
    user_text: &str,
    runner: &str,
) -> Result<BTreeMap<String, String>, String> {
    let home = std::env::var_os("HOME")
        .filter(|h| !h.is_empty())
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("/"));
    runner_env_layered_home(state_dir, core_text, user_text, runner, &home)
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

/// The `[stage]` section: the stage daemon's own switches. Today only
/// `answer_trust` — whether the stage answers Claude Code's first-use
/// folder-trust dialog itself (the default).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct StageCfg {
    pub answer_trust: bool,
}

impl Default for StageCfg {
    fn default() -> Self {
        StageCfg { answer_trust: true }
    }
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

/// The cast file's effort rule applied to Hollywood's own runner —
/// the same caps the cast reads. `hollywood setup` refuses on the
/// same rule `hollywood_in` rejects with.
pub(crate) fn effort_rule(runner: &str, effort: Option<&str>) -> Result<(), String> {
    backlot_model::caps::check_effort(runner, effort)
        .map_err(|problem| backlot_model::caps::effort_message(runner, problem))
}

/// Read `[hollywood]` from the user config; a missing file or section
/// is `Ok(None)`.
pub fn hollywood(env: &BTreeMap<String, String>) -> Result<Option<HollywoodCfg>, String> {
    hollywood_at(&config_path(env))
}

/// Read `[hollywood]` from an explicit path — the TUI's Settings
/// reload reads the file its own `panel_config_path` names (R3-M15).
pub fn hollywood_at(path: &Path) -> Result<Option<HollywoodCfg>, String> {
    match std::fs::read_to_string(path) {
        Ok(text) => hollywood_in(&text),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(e) => Err(format!("config.toml: {e}")),
    }
}

/// The `[stage]` section inside `text`: `answer_trust` is a bool,
/// absent meaning on — the default keeps the stage's own behavior.
fn stage_in(text: &str) -> Result<StageCfg, String> {
    let parsed: toml::Value = toml::from_str(text).map_err(|e| match e.span() {
        Some(span) => format!("config.toml: malformed at {}", line_col(text, span.start)),
        None => "config.toml: malformed".to_string(),
    })?;
    let Some(section) = parsed.get("stage") else {
        return Ok(StageCfg::default());
    };
    let table = section
        .as_table()
        .ok_or_else(|| "config.toml: stage is not a table".to_string())?;
    let mut cfg = StageCfg::default();
    match table.get("answer_trust") {
        None => {}
        Some(v) => {
            cfg.answer_trust = v
                .as_bool()
                .ok_or_else(|| "config.toml: stage.answer_trust is not a boolean".to_string())?;
        }
    }
    Ok(cfg)
}

/// Read `[stage]` from the user config; a missing file or section is
/// the default.
pub fn stage(env: &BTreeMap<String, String>) -> Result<StageCfg, String> {
    let path = config_path(env);
    match std::fs::read_to_string(&path) {
        Ok(text) => stage_in(&text),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(StageCfg::default()),
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

    /// No `[stage]` section is the default — answering stays on, the
    /// switch only ever turns the behavior off.
    #[test]
    fn absent_stage_section_is_the_default() {
        for text in ["", "# nothing\n", "[other]\nx = \"1\"\n"] {
            assert_eq!(stage_in(text).unwrap(), StageCfg::default());
        }
    }

    /// `answer_trust` parses as a bool; a non-boolean or a non-table
    /// `[stage]` is the error, position only.
    #[test]
    fn the_stage_section_parses_answer_trust() {
        assert!(
            !stage_in("[stage]\nanswer_trust = false\n")
                .unwrap()
                .answer_trust
        );
        assert!(
            stage_in("[stage]\nanswer_trust = true\n")
                .unwrap()
                .answer_trust
        );
        let err = stage_in("[stage]\nanswer_trust = \"no\"\n").unwrap_err();
        assert!(err.contains("answer_trust"), "{err}");
        let err = stage_in("[stage\n").unwrap_err();
        assert!(err.contains("line 1"), "{err}");
        assert!(stage_in("[stage]\nx = 1\nanswer_trust = 3\n").is_err());
        assert_eq!(stage_in("[stage]\nx = 1\n").unwrap(), StageCfg::default());
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
        assert!(err.contains("has no effort setting"), "{err}");
        let err = hollywood_in("[hollywood]\nrunner = \"fake\"\nmodel = \"m\"\n").unwrap_err();
        assert!(err.contains("takes an effort"), "{err}");
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

    /// `KEY=VALUE` lines parse; `export `, comments, blanks and
    /// quotes are tolerated, and a malformed line's error carries
    /// its number only — the value stays out of it.
    #[test]
    fn an_env_file_parses_dotenv_lines() {
        let text = concat!(
            "# a comment\n",
            "\n",
            "HTTPS_PROXY = \"http://proxy:8080\"\n",
            "export http_proxy=http://proxy:8080\n",
            "NAME = 'single quoted'\n",
            "UNQUOTED = v1\n",
        );
        let env = env_file_map(text).unwrap();
        assert_eq!(env["HTTPS_PROXY"], "http://proxy:8080");
        assert_eq!(env["http_proxy"], "http://proxy:8080");
        assert_eq!(env["NAME"], "single quoted");
        assert_eq!(env["UNQUOTED"], "v1");
        // A line with no `=` errors with its number only.
        let err = env_file_map("A = 1\nbroken line here\n").unwrap_err();
        assert!(err.contains("line 2"), "{err}");
        assert!(!err.contains("broken"), "{err}");
        // An empty key refuses too.
        assert!(env_file_map(" = 1\n").is_err());
    }

    /// The `[env_file]` table: `default` and per-runner paths as
    /// strings; absent is empty, a non-string value is the error.
    #[test]
    fn the_env_file_table_lists_paths() {
        for text in ["", "# nothing\n", "[other]\nx = \"1\"\n"] {
            assert_eq!(env_files_in(text).unwrap(), BTreeMap::new());
        }
        let text = concat!(
            "[env_file]\n",
            "default = \"~/.config/secrets/http-proxy.env\"\n",
            "agy = \"/home/user/agy.env\"\n",
        );
        let files = env_files_in(text).unwrap();
        assert_eq!(files["default"], "~/.config/secrets/http-proxy.env");
        assert_eq!(files["agy"], "/home/user/agy.env");
        assert!(env_files_in("[env_file]\ndefault = 3\n").is_err());
        assert!(env_files_in("[env_file\n").is_err());
    }

    /// The merge a launch sees: `<state>/agent.env` first, then
    /// the `default` file, then the runner's file, then the
    /// `[runner_env]` strings — each source overriding the last.
    /// A missing file is skipped; one that cannot be read is the
    /// error.
    #[test]
    fn runner_env_for_merges_files_then_strings() {
        let dir = crate::test_support::TestDir::new("backlot-cfg-env");
        std::fs::write(dir.join("agent.env"), "A = builtin\nSHARED = file\n").unwrap();
        let proxy = dir.join("proxy.env");
        std::fs::write(
            &proxy,
            "HTTPS_PROXY = http://proxy:8080\nSHARED = default\n",
        )
        .unwrap();
        let mine = dir.join("mine.env");
        std::fs::write(&mine, "MINE_ONLY = 1\nSHARED = runner\n").unwrap();
        let text = format!(
            "[env_file]\ndefault = \"{}\"\nclaude = \"{}\"\n\
             [runner_env.claude]\nSHARED = \"config\"\n",
            proxy.display(),
            mine.display()
        );
        let env = runner_env_for(dir.path(), &text, "claude").unwrap();
        assert_eq!(env["A"], "builtin");
        assert_eq!(env["HTTPS_PROXY"], "http://proxy:8080");
        assert_eq!(env["MINE_ONLY"], "1");
        assert_eq!(env["SHARED"], "config", "strings beat file entries");
        // A runner with no per-runner file gets the two lower layers.
        let env = runner_env_for(dir.path(), &text, "codex").unwrap();
        assert_eq!(env["SHARED"], "default");
        assert!(!env.contains_key("MINE_ONLY"));
    }

    /// No `agent.env` and a configured file that does not exist:
    /// both skip quietly — a file is applied only when present.
    #[test]
    fn absent_env_files_are_skipped() {
        let dir = crate::test_support::TestDir::new("backlot-cfg-env-miss");
        let env = runner_env_for(
            dir.path(),
            "[env_file]\ndefault = \"/no/such.env\"\n",
            "claude",
        )
        .unwrap();
        assert!(env.is_empty());
    }

    /// An env file that exists but will not read is the error —
    /// the path only, never the contents.
    #[test]
    fn an_unreadable_env_file_is_an_error() {
        let dir = crate::test_support::TestDir::new("backlot-cfg-env-bad");
        std::fs::write(dir.join("agent.env"), "not a kv line\n").unwrap();
        let err = runner_env_for(dir.path(), "", "claude").unwrap_err();
        assert!(err.contains("agent.env"), "{err}");
        assert!(!err.contains("kv line"), "{err}");
    }

    /// `~/` in a configured path expands under HOME.
    #[test]
    fn env_file_path_tilde_expands() {
        let dir = crate::test_support::TestDir::new("backlot-cfg-tilde");
        let home = dir.join("home");
        std::fs::create_dir_all(home.join(".config/secrets")).unwrap();
        std::fs::write(
            home.join(".config/secrets/http-proxy.env"),
            "P = from-home\n",
        )
        .unwrap();
        let text = "[env_file]\ndefault = \"~/.config/secrets/http-proxy.env\"\n";
        let env = runner_env_for_home(dir.path(), text, "claude", &home).unwrap();
        assert_eq!(env["P"], "from-home");
    }

    /// The user config's `[runner_env.<runner>]` is a lower layer than
    /// the per-core one: a PATH written there by `hollywood setup` and
    /// `studio new`'s wizard reaches the launch even with an empty
    /// per-core config.
    #[test]
    fn user_runner_env_reaches_the_launch_below_the_core_config() {
        let dir = crate::test_support::TestDir::new("backlot-cfg-user-env");
        let user = "[runner_env.cmd]\nPATH = \"/opt/nvm/bin\"\n";
        let env = runner_env_layered_home(dir.path(), "", user, "cmd", dir.path()).unwrap();
        assert_eq!(env["PATH"], "/opt/nvm/bin");
    }

    /// The per-core config wins on any key both set.
    #[test]
    fn core_runner_env_beats_the_user_config() {
        let dir = crate::test_support::TestDir::new("backlot-cfg-core-wins");
        let user = "[runner_env.cmd]\nPATH = \"/opt/nvm/bin\"\nSHADOWED = \"user\"\n";
        let core = "[runner_env.cmd]\nPATH = \"/core/bin\"\n";
        let env = runner_env_layered_home(dir.path(), core, user, "cmd", dir.path()).unwrap();
        assert_eq!(env["PATH"], "/core/bin");
        assert_eq!(env["SHADOWED"], "user", "an unshadowed user key survives");
    }

    /// Only `[runner_env]` comes from the user config — its `[env_file]`
    /// table is never read, so a user's file layer cannot sneak under
    /// the per-core one.
    #[test]
    fn the_user_configs_env_file_is_ignored() {
        let dir = crate::test_support::TestDir::new("backlot-cfg-user-env-file");
        let extra = dir.join("user.env");
        std::fs::write(&extra, "PROXY = user-file\n").unwrap();
        let user = format!(
            "[env_file]\ndefault = \"{}\"\n[runner_env.cmd]\nPATH = \"/opt/nvm/bin\"\n",
            extra.display()
        );
        let env = runner_env_layered_home(dir.path(), "", &user, "cmd", dir.path()).unwrap();
        assert_eq!(env["PATH"], "/opt/nvm/bin");
        assert!(!env.contains_key("PROXY"), "{env:?}");
    }

    /// A malformed user config refuses like a malformed per-core one:
    /// the position only, never a value beside it.
    #[test]
    fn a_malformed_user_config_refuses_with_position_only() {
        let dir = crate::test_support::TestDir::new("backlot-cfg-user-bad");
        let user = "[runner_env.cmd]\nHTTPS_PROXY = http://user:pw@host\n";
        let err = runner_env_layered_home(dir.path(), "", user, "cmd", dir.path()).unwrap_err();
        assert!(err.contains("malformed at line 2"), "{err}");
        assert!(err.contains("column"), "{err}");
        assert!(!err.contains("pw"), "{err}");
        assert!(!err.contains("http"), "{err}");
    }
}
