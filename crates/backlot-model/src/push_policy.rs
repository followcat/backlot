//! Studio push policy and recorded remote URLs (push-perms spec §2).
//!
//! `studio.created.push` freezes the studio's protected branches, tag/default-branch
//! protection flags, and the remote push URLs recorded from the bound repository
//! (or workspace member repositories) at creation time.

use serde::{Deserialize, Serialize};

/// One remote push URL recorded from a bound repository at `studio.created`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RecordedRemote {
    /// Bound member repository path for workspace studios; `None` for single-repo studios.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub repo: Option<String>,
    pub name: String,
    pub url: String,
}

/// Studio-level outward push policy recorded on `studio.created` and configurable
/// via `[push]` in `cast.toml`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PushPolicy {
    #[serde(default = "default_protected_branches")]
    pub protected_branches: Vec<String>,
    #[serde(default = "default_true")]
    pub protect_tags: bool,
    #[serde(default = "default_true")]
    pub protect_default_branch: bool,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub remotes: Vec<RecordedRemote>,
}

/// Default protected branch patterns for new and legacy studios.
pub fn default_protected_branches() -> Vec<String> {
    vec![
        "main".into(),
        "master".into(),
        "develop".into(),
        "release*".into(),
    ]
}

fn default_true() -> bool {
    true
}

impl Default for PushPolicy {
    fn default() -> Self {
        PushPolicy {
            protected_branches: default_protected_branches(),
            protect_tags: true,
            protect_default_branch: true,
            remotes: Vec::new(),
        }
    }
}

impl PushPolicy {
    /// True when all fields equal `PushPolicy::default()` (used for `skip_serializing_if`).
    pub fn is_default(&self) -> bool {
        *self == Self::default()
    }

    /// Return the first pattern in `protected_branches` that matches `branch`
    /// (after stripping an optional `refs/heads/` prefix), if any.
    pub fn matched_protected_branch(&self, branch: &str) -> Option<&str> {
        let bare = branch.strip_prefix("refs/heads/").unwrap_or(branch);
        self.protected_branches
            .iter()
            .find(|pat| glob_match(pat, bare))
            .map(String::as_str)
    }

    /// Whether `url` matches one of `remotes` for the caller's repo.
    pub fn has_recorded_url<F>(&self, url: &str, mut repo_matches: F) -> bool
    where
        F: FnMut(&str) -> bool,
    {
        self.remotes
            .iter()
            .any(|r| r.url == url && r.repo.as_deref().is_none_or(&mut repo_matches))
    }
}

/// Match `text` against a simple glob pattern supporting `*` (0+ chars) and `?` (1 char).
pub fn glob_match(pattern: &str, text: &str) -> bool {
    let p: Vec<char> = pattern.chars().collect();
    let t: Vec<char> = text.chars().collect();
    let (mut pi, mut ti) = (0usize, 0usize);
    let mut star_pi: Option<usize> = None;
    let mut star_ti = 0usize;

    while ti < t.len() {
        if pi < p.len() && (p[pi] == '?' || p[pi] == t[ti]) {
            pi += 1;
            ti += 1;
        } else if pi < p.len() && p[pi] == '*' {
            star_pi = Some(pi);
            pi += 1;
            star_ti = ti;
        } else if let Some(sp) = star_pi {
            pi = sp + 1;
            star_ti += 1;
            ti = star_ti;
        } else {
            return false;
        }
    }
    while pi < p.len() && p[pi] == '*' {
        pi += 1;
    }
    pi == p.len()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_policy_and_glob_matching() {
        let p = PushPolicy::default();
        assert_eq!(p.matched_protected_branch("main"), Some("main"));
        assert_eq!(
            p.matched_protected_branch("refs/heads/release/v2"),
            Some("release*")
        );
        assert_eq!(p.matched_protected_branch("feat/login"), None);
        assert!(p.protect_tags);
        assert!(p.protect_default_branch);
        assert!(p.remotes.is_empty());
    }

    #[test]
    fn legacy_json_without_push_deserializes_to_default_with_empty_remotes() {
        let p: PushPolicy = serde_json::from_str("{}").unwrap();
        assert_eq!(p, PushPolicy::default());
        assert!(p.remotes.is_empty());
    }
}
