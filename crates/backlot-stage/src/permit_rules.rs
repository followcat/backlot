//! Derivation of the narrow rules a remembered permission stores.
//! The shell splitter is the guard's own — compound commands are covered
//! only segment by segment, and any ungrantable segment makes the whole
//! prompt once-only.

use crate::guard_shell_lex::split_shell_segments_checked;

/// One derived rule and whether it reaches outside the machine.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DerivedRule {
    pub rule: String,
    pub outward: bool,
}

/// What a prompt command can be remembered as.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Derivation {
    /// Every segment yielded a rule. Several rules mean a compound command.
    Rules(Vec<DerivedRule>),
    /// The command can be allowed once but never remembered; the string
    /// is the person-facing reason.
    OnceOnly(String),
}

const WRAPPERS: &[&str] = &[
    "sh", "bash", "zsh", "dash", "ash", "node", "perl", "ruby", "env", "xargs", "flock", "timeout",
    "nice", "nohup", "sudo", "eval", "exec", "time", "watch",
];
const DESTRUCTIVE: &[&str] = &["rm", "mv", "dd", "chmod", "chown", "kill", "pkill"];
const MULTI: &[&str] = &[
    "git", "cargo", "npm", "docker", "gh", "hc", "backlot", "kubectl",
];
const CONTROL: &[&str] = &[
    "for", "do", "done", "if", "then", "else", "elif", "fi", "while", "until", "case", "esac",
    "select", "function", "in",
];
const CLOUD: &[&str] = &[
    "colab",
    "gcloud",
    "aws",
    "az",
    "azure",
    "ibmcloud",
    "doctl",
    "hcloud",
    "vultr-cli",
];

fn base(word: &str) -> &str {
    word.rsplit('/').next().unwrap_or(word)
}

fn once(why: impl Into<String>) -> Derivation {
    Derivation::OnceOnly(why.into())
}

fn assignment(word: &str) -> bool {
    let Some((name, _)) = word.split_once('=') else {
        return false;
    };
    !name.is_empty()
        && name
            .chars()
            .next()
            .is_some_and(|c| c.is_ascii_alphabetic() || c == '_')
        && name.chars().all(|c| c.is_ascii_alphanumeric() || c == '_')
}

fn redirection(word: &str) -> Option<bool> {
    // `true` means benign for rule derivation: stderr discarded or one
    // descriptor duplicated into another. Every other redirection is
    // treated as once-only because a persistent prefix cannot bound it.
    let w = word
        .strip_prefix("2>")
        .or_else(|| word.strip_prefix("1>"))
        .unwrap_or(word);
    if w == "/dev/null" || w.starts_with("&1") || w.starts_with("&2") || w.starts_with("&-") {
        return Some(true);
    }
    if word.starts_with('>')
        || word.starts_with('<')
        || word.contains(">") && word.ends_with("/dev/null")
    {
        return Some(false);
    }
    if word.starts_with("2>&1") || word.starts_with("1>&2") || word.starts_with("2>/dev/null") {
        return Some(true);
    }
    if word.starts_with("2>") || word.starts_with("1>") || word == ">" || word == "<" {
        return Some(false);
    }
    None
}

fn exec_action(prog: &str, args: &[String]) -> bool {
    if prog == "find" {
        return args.iter().any(|a| {
            matches!(
                a.as_str(),
                "-exec" | "-execdir" | "-delete" | "-ok" | "-okdir"
            )
        });
    }
    if prog == "fd" {
        return args.iter().any(|a| {
            matches!(a.as_str(), "-x" | "--exec" | "-X" | "--exec-batch")
                || a.starts_with("--exec=")
                || a.starts_with("--exec-batch=")
        });
    }
    false
}

fn git_rule(words: &[String]) -> Result<String, String> {
    let mut i = 1usize;
    let mut prefix = vec!["git".to_string()];
    // Preserve repository-selecting globals: they are part of the narrow
    // shape the person saw. Config/transport overrides are the guard's
    // territory and are never normalized away.
    while i < words.len() {
        match words[i].as_str() {
            "-C" if i + 1 < words.len() => {
                prefix.push("-C".into());
                prefix.push(words[i + 1].clone());
                i += 2;
            }
            "--git-dir" | "--work-tree" => {
                prefix.push(words[i].clone());
                if i + 1 < words.len() {
                    prefix.push(words[i + 1].clone());
                }
                i += 2;
            }
            "-c" | "--config-env" => {
                return Err("git config overrides can change what the command does".into());
            }
            w if w.starts_with("--git-dir=") || w.starts_with("--work-tree=") => {
                prefix.push(w.into());
                i += 1;
            }
            w if w.starts_with('-') => {
                prefix.push(w.into());
                i += 1;
            }
            _ => break,
        }
    }
    let Some(sub) = words.get(i) else {
        return Err("git without a subcommand is too broad to remember".into());
    };
    if matches!(sub.as_str(), "reset" | "clean" | "push") {
        return Err(format!(
            "git {sub} is destructive or outward and is never remembered"
        ));
    }
    prefix.push(sub.clone());
    // `branch` and `remote` are multi-mode subcommands: the mode word or
    // flag is part of the rule, never the bare subcommand.
    if matches!(sub.as_str(), "branch" | "remote" | "config") {
        let mode = words
            .get(i + 1)
            .ok_or_else(|| format!("git {sub} without a mode is too broad to remember"))?;
        if mode.starts_with('-') || sub != "branch" {
            prefix.push(mode.clone());
        } else {
            return Err(format!("git {sub} without a mode is too broad to remember"));
        }
    }
    Ok(prefix.join(" "))
}

fn subcommand_rule(words: &[String], prog: &str) -> Result<String, String> {
    let sub = words
        .get(1)
        .ok_or_else(|| format!("{prog} without a subcommand is too broad to remember"))?;
    if sub.starts_with('-') {
        return Err(format!(
            "{prog} without a subcommand is too broad to remember"
        ));
    }
    let mut out = format!("{prog} {sub}");
    if prog == "npm" && sub == "run" {
        let script = words
            .get(2)
            .ok_or_else(|| "npm run needs its script in the rule".to_string())?;
        out.push(' ');
        out.push_str(script);
    }
    Ok(out)
}

fn outward(prog: &str, words: &[String]) -> bool {
    if matches!(
        prog,
        "curl" | "wget" | "huggingface-cli" | "hf" | "ssh" | "scp"
    ) || CLOUD.contains(&prog)
    {
        return true;
    }
    let text = words.join(" ");
    matches!(prog, "pip" | "pip3" | "uv" | "npm" | "pnpm" | "yarn") && text.contains(" install")
        || prog == "git"
            && matches!(
                words.get(1).map(String::as_str),
                Some("clone" | "fetch" | "pull")
            )
        || matches!(prog, "npm" | "cargo" | "pnpm" | "yarn") && text.contains(" publish")
        || prog == "rsync" && words.iter().skip(1).any(|a| a.contains(':'))
}

fn segment_rule(words: &[String]) -> Result<DerivedRule, String> {
    if words.is_empty() {
        return Err("an empty shell segment cannot be remembered".into());
    }
    if CONTROL.contains(&words[0].as_str()) {
        return Err("shell loops and conditionals run different commands each time".into());
    }
    let mut clean = Vec::new();
    for w in words {
        if assignment(w) {
            return Err("environment assignments can change what the command runs as".into());
        }
        match redirection(w) {
            Some(true) => continue,
            Some(false) => return Err("file redirection is outside a command-stem grant".into()),
            None => clean.push(w.clone()),
        }
    }
    if clean.is_empty() {
        return Err("an empty shell segment cannot be remembered".into());
    }
    let prog = base(&clean[0]).to_string();
    if prog.starts_with("python") || WRAPPERS.contains(&prog.as_str()) {
        return Err(format!(
            "`{prog}` runs arbitrary code and cannot be remembered"
        ));
    }
    if DESTRUCTIVE.contains(&prog.as_str()) {
        return Err(format!("`{prog}` is destructive and cannot be remembered"));
    }
    if exec_action(&prog, &clean[1..]) {
        return Err(format!(
            "`{prog}` with an exec or delete action cannot be remembered"
        ));
    }
    let out = outward(&prog, &clean);
    let rule = match prog.as_str() {
        "git" => git_rule(&clean)?,
        "du" | "sed" | "sort" => {
            let mut parts = vec![prog.clone()];
            for w in &clean[1..] {
                if w.starts_with('-') {
                    parts.push(w.clone());
                } else {
                    break;
                }
            }
            parts.join(" ")
        }
        "tar" => {
            let mode = clean
                .get(1)
                .ok_or_else(|| "tar without a mode is too broad to remember".to_string())?;
            if mode.starts_with("--") {
                return Err("long tar modes are too broad to remember".into());
            }
            format!("tar {}", mode.trim_start_matches('-'))
        }
        "pip" | "pip3" => {
            let sub = clean
                .get(1)
                .ok_or_else(|| "pip without a subcommand is too broad to remember".to_string())?;
            format!("{prog} {sub}")
        }
        "uv" if clean.get(1).map(String::as_str) == Some("pip") => {
            let sub = clean.get(2).ok_or_else(|| {
                "uv pip without a subcommand is too broad to remember".to_string()
            })?;
            format!("uv pip {sub}")
        }
        p if MULTI.contains(&p) => subcommand_rule(&clean, &prog)?,
        _ => {
            let mut parts = vec![prog.clone()];
            for w in &clean[1..] {
                if w.starts_with('-') {
                    parts.push(w.clone());
                } else {
                    break;
                }
            }
            parts.join(" ")
        }
    };
    Ok(DerivedRule { rule, outward: out })
}

/// Derive the rules for one complete shell command line. Compound
/// commands split on `;`, `&&`, `||` and pipes; every segment must yield
/// a rule or the whole command is once-only.
pub fn derive(command: &str) -> Derivation {
    let parsed = match split_shell_segments_checked(command) {
        Ok(p) => p,
        Err(e) => return once(format!("unparseable shell syntax: {e}")),
    };
    if !parsed.nested.is_empty() || !parsed.heredocs.is_empty() {
        return once("command substitution and heredocs run arbitrary code");
    }
    let mut rules = Vec::new();
    for segment in &parsed.segments {
        match segment_rule(segment) {
            Ok(r) => rules.push(r),
            Err(why) => return once(why),
        }
    }
    if rules.is_empty() {
        once("the prompt names no shell command")
    } else {
        Derivation::Rules(rules)
    }
}

/// The rules covering a command, or why it cannot be covered. This is
/// the same derivation used for grants, so a remembered rule and the
/// prompt it came from compare in one normalization.
pub fn coverage(command: &str) -> Result<Vec<DerivedRule>, String> {
    match derive(command) {
        Derivation::Rules(r) => Ok(r),
        Derivation::OnceOnly(why) => Err(why),
    }
}
