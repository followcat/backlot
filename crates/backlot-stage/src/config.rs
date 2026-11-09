//! `<state base>/config.toml` parsing: `[runner_env.<runner>]` string
//! maps hold per-runner settings a role terminal gets through
//! `terminal.run`'s env — a proxy for Devin, say. Values never reach
//! labels, the ledger, the summary or logs.

use std::collections::BTreeMap;

/// `line L column C` for a byte offset — the only part of a parse
/// error that may be reported: the parser's message quotes the
/// offending line, and a config file may hold secrets (a proxy URL
/// with credentials, say) that must never reach the ledger.
fn line_col(text: &str, pos: usize) -> String {
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
}
