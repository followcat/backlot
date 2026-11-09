//! `<state base>/config.toml` parsing: `[runner_env.<runner>]` string
//! maps hold per-runner settings a role terminal gets through
//! `terminal.run`'s env — a proxy for Devin, say. Values never reach
//! labels, the ledger, the summary or logs.

use std::collections::BTreeMap;

/// The `[runner_env.<runner>]` map inside `text`. A missing section
/// is an empty map; malformed TOML or a non-string value is the
/// error detail a `config malformed` attention carries.
pub fn runner_env_in(text: &str, runner: &str) -> Result<BTreeMap<String, String>, String> {
    let parsed: toml::Value = toml::from_str(text).map_err(|e| format!("config.toml: {e}"))?;
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
}
