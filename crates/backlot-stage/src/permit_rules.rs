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
    "sh",
    "bash",
    "zsh",
    "dash",
    "ash",
    "node",
    "perl",
    "ruby",
    "env",
    "xargs",
    "flock",
    "timeout",
    "nice",
    "nohup",
    "sudo",
    "eval",
    "exec",
    "time",
    "watch",
    "awk",
    "gawk",
    "mawk",
    "nawk",
    "npx",
    "pnpx",
    "bunx",
    "uvx",
    "bun",
    "deno",
    "php",
    "lua",
    "Rscript",
    "osascript",
];
const DESTRUCTIVE: &[&str] = &["rm", "mv", "dd", "chmod", "chown", "kill", "pkill"];
const MULTI: &[&str] = &[
    "git", "cargo", "npm", "docker", "gh", "hc", "backlot", "kubectl", "uv", "go", "pnpm", "yarn",
    "make", "just", "poetry",
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

pub(crate) fn assignment(word: &str) -> bool {
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

fn short_cluster_has(arg: &str, flag: char) -> bool {
    arg.starts_with('-') && !arg.starts_with("--") && arg[1..].contains(flag)
}

fn plain_sed_print(script: &str) -> bool {
    !script.trim().is_empty()
        && script.split([';', '\n']).all(|piece| {
            let piece = piece.trim();
            !piece.is_empty()
                && piece.strip_suffix('p').is_some_and(|address| {
                    address
                        .chars()
                        .all(|c| c.is_ascii_digit() || c == ',' || c == '$')
                })
        })
}

fn sed_write_or_exec(args: &[String]) -> bool {
    let mut scripts = Vec::new();
    let mut positional_script = false;
    let mut i = 0;
    while i < args.len() {
        let a = args[i].as_str();
        if a == "-e" || a == "--expression" {
            let Some(script) = args.get(i + 1) else {
                return true;
            };
            scripts.push(script.clone());
            i += 2;
            continue;
        }
        if let Some(script) = a.strip_prefix("--expression=") {
            scripts.push(script.to_string());
            i += 1;
            continue;
        }
        if a.starts_with('-') && !a.starts_with("--") {
            let cluster = &a[1..];
            // In-place editing and script files are never safe. A clustered
            // `-e` cannot be separated reliably from the other short flags,
            // so only the unclustered `-e script` forms are inspectable.
            if cluster.contains('i') || cluster.contains('f') {
                return true;
            }
            if cluster.contains('e') {
                if !a.starts_with("-e") {
                    return true;
                }
                if cluster.len() > 1 {
                    scripts.push(cluster[1..].to_string());
                }
                i += 1;
                continue;
            }
        }
        if a == "-f"
            || a == "--file"
            || a.starts_with("--file=")
            || a == "-i"
            || a == "--in-place"
            || a.starts_with("--in-place=")
        {
            return true;
        }
        if !a.starts_with('-') && !positional_script {
            positional_script = true;
            scripts.push(a.to_string());
        }
        i += 1;
    }
    if scripts.is_empty() {
        return false;
    }
    // Only plain line-address prints are reliably side-effect free (`-n '1,160p'`).
    // Every other sed program is conservatively once-only: `e` executes a shell
    // command and `w`/`W` write files, including inside addresses and branches.
    !scripts.iter().all(|script| plain_sed_print(script))
}

pub(crate) fn exec_action(prog: &str, args: &[String]) -> bool {
    match prog {
        "find" => args.iter().any(|a| {
            matches!(
                a.as_str(),
                "-exec" | "-execdir" | "-delete" | "-ok" | "-okdir"
            ) || a.starts_with("-fprint")
        }),
        "fd" => args.iter().any(|a| {
            matches!(a.as_str(), "--exec" | "--exec-batch")
                || short_cluster_has(a, 'x')
                || short_cluster_has(a, 'X')
                || a.starts_with("--exec=")
                || a.starts_with("--exec-batch=")
        }),
        "rg" => args.iter().any(|a| a == "--pre" || a.starts_with("--pre=")),
        "sort" => args.iter().any(|a| {
            a == "--compress-program"
                || a.starts_with("--compress-program=")
                || short_cluster_has(a, 'o')
                || a == "--output"
                || a.starts_with("--output=")
        }),
        "tar" => {
            args.first().is_some_and(|mode| {
                !mode.starts_with('-') && (mode.contains('I') || mode.contains('F'))
            }) || args.iter().any(|a| {
                a == "--to-command"
                    || a.starts_with("--to-command=")
                    || short_cluster_has(a, 'I')
                    || a == "--use-compress-program"
                    || a.starts_with("--use-compress-program=")
                    || a == "--checkpoint-action"
                    || a.starts_with("--checkpoint-action=")
                    || short_cluster_has(a, 'F')
                    || a == "--info-script"
                    || a.starts_with("--info-script=")
                    || a == "--new-volume-script"
                    || a.starts_with("--new-volume-script=")
            })
        }
        "sed" => sed_write_or_exec(args),
        "git" => args.iter().any(|a| {
            a == "--ext-diff"
                || a == "--output"
                || a.starts_with("--output=")
                || a == "--exec"
                || a.starts_with("--exec=")
                || a == "--upload-pack"
                || a.starts_with("--upload-pack=")
                || a == "--receive-pack"
                || a.starts_with("--receive-pack=")
        }),
        _ => false,
    }
}

fn git_rule(words: &[String]) -> Result<String, String> {
    let mut i = 1usize;
    let mut prefix = vec![words[0].clone()];
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
    if matches!(
        (prog, sub.as_str()),
        ("npm", "exec")
            | ("npm", "x")
            | ("pnpm", "exec")
            | ("pnpm", "dlx")
            | ("yarn", "exec")
            | ("yarn", "dlx")
            | ("uv", "run")
            | ("uv", "tool")
    ) {
        return Err(format!(
            "{prog} {sub} runs arbitrary code and cannot be remembered"
        ));
    }
    let mut out = format!("{} {sub}", words[0]);
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
    let prog_base = base(&clean[0]).to_string();
    let prog = clean[0].clone();
    if prog_base.starts_with("python") || WRAPPERS.contains(&prog_base.as_str()) {
        return Err(format!(
            "`{prog}` runs arbitrary code and cannot be remembered"
        ));
    }
    if DESTRUCTIVE.contains(&prog_base.as_str()) {
        return Err(format!("`{prog}` is destructive and cannot be remembered"));
    }
    if exec_action(&prog_base, &clean[1..]) {
        return Err(format!(
            "`{prog}` with an exec or delete action cannot be remembered"
        ));
    }
    let out = outward(&prog_base, &clean);
    let rule = match prog_base.as_str() {
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
            format!("{} {}", prog, mode.trim_start_matches('-'))
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
            format!("{} pip {sub}", clean[0])
        }
        p if MULTI.contains(&p) => subcommand_rule(&clean, &prog_base)?,
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

/// What an attention detail asks for, after the runner tool prefix is
/// stripped. The detail is the ledger's uncapped tool line, so the rule
/// shown in the client is the rule the server derives from the same text.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AttentionTarget {
    Shell(Derivation),
    Edit(String),
    Unknown,
}

const EDIT_TOOLS: &[&str] = &[
    "Edit",
    "Write",
    "NotebookEdit",
    "edit",
    "write",
    "create_file",
    "str_replace_editor",
];
const SHELL_TOOLS: &[&str] = &[
    "Bash",
    "Shell",
    "run_command",
    "execute_shell_command",
    "terminal",
    "Command",
];

/// Parse `attention_detail`/`attention_target`: either `<tool>: <target>`
/// or a bare shell command. An empty or unbound detail is unknown.
pub fn attention_target(detail: &str) -> AttentionTarget {
    let detail = detail.trim();
    if detail.is_empty() || detail.starts_with("unbound prompt") {
        return AttentionTarget::Unknown;
    }
    let (tool, body) = detail
        .split_once(": ")
        .map(|(t, b)| (Some(t.trim()), b.trim()))
        .unwrap_or((None, detail));
    if body.is_empty() {
        return AttentionTarget::Unknown;
    }
    if tool.is_some_and(|t| EDIT_TOOLS.contains(&t)) {
        return AttentionTarget::Edit(body.to_string());
    }
    if tool.is_some_and(|t| !SHELL_TOOLS.contains(&t) && t.chars().any(|c| c.is_whitespace())) {
        return AttentionTarget::Unknown;
    }
    AttentionTarget::Shell(derive(body))
}
