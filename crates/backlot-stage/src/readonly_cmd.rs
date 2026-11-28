//! Whether a complete shell command line is provably read-only for a
//! role. The guard lexer owns the shell syntax; this module only
//! interprets the resulting segments.

use std::path::{Path, PathBuf};

use crate::guard_shell_lex::split_shell_segments_checked;

fn redirection(word: &str) -> Option<Result<usize, &'static str>> {
    let digits = word.len() - word.trim_start_matches(|c: char| c.is_ascii_digit()).len();
    if digits == word.len() {
        return None;
    }
    let w = &word[digits..];
    if w.is_empty() {
        return None;
    }
    if w.starts_with(">&") || w.starts_with("<&") {
        let target = &w[2..];
        if target == "-" || (!target.is_empty() && target.bytes().all(|b| b.is_ascii_digit())) {
            return Some(Ok(0));
        }
        return Some(Err(
            "only file-descriptor duplication redirections are allowed",
        ));
    }
    if w == ">" || w == "<" || w == ">>" {
        return Some(Ok(1));
    }
    if let Some(target) = w.strip_prefix(">>") {
        if target == "/dev/null" {
            return Some(Ok(0));
        }
        return Some(Err("file redirection is not read-only"));
    }
    if let Some(target) = w.strip_prefix('>').or_else(|| w.strip_prefix('<')) {
        if target == "/dev/null" {
            return Some(Ok(0));
        }
        return Some(Err("file redirection is not read-only"));
    }
    None
}

fn redirection_ok(word: &str, next: Option<&str>) -> Result<usize, &'static str> {
    match redirection(word) {
        None => Err("not a redirection"),
        Some(Ok(0)) => Ok(0),
        Some(Ok(_)) => {
            if next == Some("/dev/null") {
                Ok(1)
            } else {
                Err("file redirection is not read-only")
            }
        }
        Some(Err(e)) => Err(e),
    }
}

fn shell_words(words: &[String]) -> Result<Vec<String>, &'static str> {
    let mut out = Vec::new();
    let mut i = 0;
    while i < words.len() {
        let w = words[i].as_str();
        if redirection(w).is_some() {
            let consume = redirection_ok(w, words.get(i + 1).map(String::as_str))?;
            i += 1 + consume;
            continue;
        }
        // The shared lexer keeps redirection characters inside the surrounding
        // word (`cat f>out` stays one word). Quotes are already gone here, so
        // an angle bracket that is not the whole recognised redirection form is
        // refused rather than risk missing a write. This is intentionally
        // conservative for patterns containing `<` or `>`.
        if w.contains('<') || w.contains('>') {
            return Err("an unrecognised redirection is not read-only");
        }
        // The shared lexer removes quote information. Since it cannot
        // distinguish a literal single-quoted dollar from parameter
        // expansion, every dollar or backtick in a shell word is refused.
        if w.contains('$') || w.contains('`') {
            return Err("parameter expansion is not read-only-checkable");
        }
        out.push(words[i].clone());
        i += 1;
    }
    if out.is_empty() {
        return Err("an empty shell segment is not a command");
    }
    if out.iter().any(|w| crate::permit_rules::assignment(w)) {
        return Err("environment assignments are not read-only commands");
    }
    Ok(out)
}

/// Check the complete command line. `cwd` is the role's effective work
/// directory; `roots` are the role worktree, studio repository, studio
/// directory and role temporary directory.
pub(crate) fn check_command(
    line: &str,
    cwd: &Path,
    roots: &[PathBuf],
    home: Option<&Path>,
) -> Result<(), &'static str> {
    crate::readonly_path::check_cwd(cwd, roots)?;
    let parsed = split_shell_segments_checked(line).map_err(|_| "unparseable shell syntax")?;
    if parsed.backgrounded {
        return Err("backgrounding is not a read-only command form");
    }
    if !parsed.nested.is_empty() {
        return Err("command or process substitution is not read-only");
    }
    if !parsed.heredocs.is_empty() {
        return Err("heredocs are not read-only command forms");
    }
    if parsed.segments.is_empty() {
        return Err("an empty command is not read-only");
    }
    for segment in &parsed.segments {
        let words = shell_words(segment)?;
        crate::readonly_args::check_program(&words, cwd, roots, home)?;
    }
    Ok(())
}

/// The role's read-only roots: its worktree, the studio repository, the
/// studio state directory and `roles/<role>/tmp`.
pub(crate) fn roots(
    studio_dir: &Path,
    repo: &Path,
    role_dir: &Path,
    work_area: &Path,
) -> Vec<PathBuf> {
    vec![
        work_area.to_path_buf(),
        repo.to_path_buf(),
        studio_dir.to_path_buf(),
        role_dir.join("tmp"),
    ]
}
