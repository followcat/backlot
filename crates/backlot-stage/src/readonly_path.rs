//! Path resolution for the provably read-only shell classifier.

use std::path::{Component, Path, PathBuf};

/// A glob whose expansion is larger than this is refused rather than
/// making a pre-tool hook enumerate unbounded output.
const GLOB_MATCH_CAP: usize = 2048;

fn expand_tilde(arg: &str, home: Option<&Path>) -> Result<PathBuf, &'static str> {
    if arg == "~" || arg.starts_with("~/") {
        let home = home.ok_or("a tilde path cannot be resolved")?;
        return Ok(home.join(
            arg.strip_prefix('~')
                .unwrap_or_default()
                .trim_start_matches('/'),
        ));
    }
    if arg.starts_with('~') {
        return Err("another user's tilde path is outside the role");
    }
    Ok(PathBuf::from(arg))
}

/// Resolve `path` component by component. Every existing prefix is
/// canonicalized, so a symlink is compared by its target; a missing
/// tail is compared against its deepest existing parent.
pub(crate) fn resolve_path(path: &Path) -> PathBuf {
    let mut cur = if path.is_absolute() {
        PathBuf::from("/")
    } else {
        return path.to_path_buf();
    };
    let mut missing = false;
    for comp in path.components() {
        match comp {
            Component::RootDir | Component::CurDir => {}
            Component::ParentDir => {
                if missing {
                    cur.pop();
                } else {
                    cur.push("..");
                    if cur.exists() {
                        cur = cur.canonicalize().unwrap_or_else(|_| cur.clone());
                    } else {
                        cur.pop();
                        cur.pop();
                        missing = true;
                    }
                }
            }
            Component::Normal(part) => {
                cur.push(part);
                if !missing && cur.exists() {
                    cur = cur.canonicalize().unwrap_or_else(|_| cur.clone());
                } else {
                    missing = true;
                }
            }
            Component::Prefix(_) => {}
        }
    }
    cur
}

fn resolve_arg(arg: &str, cwd: &Path, home: Option<&Path>) -> Result<PathBuf, &'static str> {
    if arg.is_empty() || arg.contains('\0') {
        return Err("an empty or NUL path cannot be checked");
    }
    let raw = expand_tilde(arg, home)?;
    let joined = if raw.is_absolute() {
        raw
    } else {
        cwd.join(raw)
    };
    if !joined.is_absolute() {
        return Err("a relative path cannot be checked");
    }
    Ok(resolve_path(&joined))
}

fn canonical_root(root: &Path) -> PathBuf {
    let absolute = if root.is_absolute() {
        root.to_path_buf()
    } else {
        std::env::current_dir()
            .unwrap_or_else(|_| PathBuf::from("/"))
            .join(root)
    };
    resolve_path(&absolute)
}

fn inside_roots(path: &Path, roots: &[PathBuf]) -> bool {
    roots.iter().any(|root| {
        let root = canonical_root(root);
        path == root || path.starts_with(root)
    })
}

fn check_resolved(resolved: &Path, roots: &[PathBuf]) -> Result<(), &'static str> {
    if crate::permit_cmd::secret_path(resolved) {
        return Err("path names a secret");
    }
    if inside_roots(resolved, roots) {
        Ok(())
    } else {
        Err("path is outside the role's roots")
    }
}

fn globbed(word: &str) -> bool {
    word.bytes().any(|b| matches!(b, b'*' | b'?' | b'['))
}

/// Match one pathname component against a glob component. `*` and `?`
/// keep shell meaning. A bracket expression is deliberately widened to
/// any one character: that can check extra paths, never fewer, when the
/// exact class grammar is not needed for a conservative permission proof.
fn wildcard_match(pattern: &str, name: &str) -> bool {
    #[derive(Clone, Copy, PartialEq, Eq)]
    enum Token {
        Star,
        Any,
        Literal(char),
    }
    let mut tokens = Vec::new();
    let chars: Vec<char> = pattern.chars().collect();
    let mut i = 0;
    while i < chars.len() {
        match chars[i] {
            '*' => tokens.push(Token::Star),
            '?' => tokens.push(Token::Any),
            '[' => {
                let mut end = i + 1;
                while end < chars.len() && chars[end] != ']' {
                    end += 1;
                }
                tokens.push(Token::Any);
                if end < chars.len() {
                    i = end;
                }
            }
            c => tokens.push(Token::Literal(c)),
        }
        i += 1;
    }
    let name: Vec<char> = name.chars().collect();
    let (mut p, mut n) = (0, 0);
    let mut star = None;
    while n < name.len() {
        if p < tokens.len() && (tokens[p] == Token::Any || tokens[p] == Token::Literal(name[n])) {
            p += 1;
            n += 1;
        } else if p < tokens.len() && tokens[p] == Token::Star {
            star = Some((p, n));
            p += 1;
        } else if let Some((sp, sn)) = star {
            p = sp + 1;
            n = sn + 1;
            star = Some((sp, n));
        } else {
            return false;
        }
    }
    while p < tokens.len() && tokens[p] == Token::Star {
        p += 1;
    }
    p == tokens.len()
}

/// Expand one glob component at a time. Shell globbing only reads
/// directory names here; every resulting path is still canonicalized and
/// root-checked by the caller. An unreadable existing directory is a
/// refusal rather than a silent miss.
fn expand_glob(
    arg: &str,
    cwd: &Path,
    roots: &[PathBuf],
    home: Option<&Path>,
) -> Result<Vec<PathBuf>, &'static str> {
    let raw = expand_tilde(arg, home)?;
    let mut bases = vec![if raw.is_absolute() {
        PathBuf::from("/")
    } else {
        cwd.to_path_buf()
    }];
    for comp in raw.components() {
        match comp {
            Component::RootDir | Component::CurDir => {}
            Component::ParentDir => {
                bases = bases.iter().map(|b| b.join("..")).collect();
            }
            Component::Normal(part) => {
                let pattern = part.to_string_lossy();
                if !globbed(&pattern) {
                    bases = bases.iter().map(|b| b.join(part)).collect();
                    continue;
                }
                let mut next = Vec::new();
                for base in &bases {
                    if !base.exists() {
                        continue;
                    }
                    check_resolved(&resolve_path(base), roots)?;
                    let entries =
                        std::fs::read_dir(base).map_err(|_| "a glob directory cannot be read")?;
                    for entry in entries {
                        let entry = entry.map_err(|_| "a glob directory entry cannot be read")?;
                        let name = entry.file_name();
                        if wildcard_match(&pattern, &name.to_string_lossy()) {
                            next.push(base.join(&name));
                            if next.len() > GLOB_MATCH_CAP {
                                return Err("a glob expands too broadly to check");
                            }
                        }
                    }
                }
                bases = next;
            }
            Component::Prefix(_) => {}
        }
    }
    Ok(bases)
}

/// The command's real current directory must itself be inside the
/// role's roots. Even a command with no explicit file argument can read
/// this directory implicitly (`ls`, `rg` or `find` without a start).
pub(crate) fn check_cwd(cwd: &Path, roots: &[PathBuf]) -> Result<(), &'static str> {
    if !cwd.is_absolute() {
        return Err("the command cwd is not absolute");
    }
    let resolved = resolve_path(cwd);
    if crate::permit_cmd::secret_path(&resolved) {
        return Err("the command cwd names a secret");
    }
    if inside_roots(&resolved, roots) {
        Ok(())
    } else {
        Err("the command cwd is outside the role's roots")
    }
}

/// Check one filesystem path argument against the role's roots and the
/// conservative secret-path table.
pub(crate) fn check_path(
    arg: &str,
    cwd: &Path,
    roots: &[PathBuf],
    home: Option<&Path>,
) -> Result<(), &'static str> {
    if arg.is_empty() || arg.contains('\0') {
        return Err("an empty or NUL path cannot be checked");
    }
    if arg.contains('{') || arg.contains('}') {
        return Err("brace expansion cannot be checked literally");
    }
    if globbed(arg) {
        let matches = expand_glob(arg, cwd, roots, home)?;
        if !matches.is_empty() {
            for path in matches {
                check_resolved(&resolve_path(&path), roots)?;
            }
            return Ok(());
        }
        // No match: the shell passes the pattern through literally, so
        // the literal spelling is the path to check.
    }
    let resolved = resolve_arg(arg, cwd, home)?;
    check_resolved(&resolved, roots)
}

/// Whether a word is shaped like a path even in a position that may
/// instead be a pattern, revision or operand.
pub(crate) fn obvious_path(word: &str) -> bool {
    word == "~"
        || word == "."
        || word == ".."
        || word.starts_with('/')
        || word.starts_with("~/")
        || word.starts_with("./")
        || word.starts_with("../")
        || word.contains('/')
}
