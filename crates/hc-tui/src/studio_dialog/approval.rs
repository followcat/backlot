//! Permission approval choices: once, remembered for the role or studio,
//! or denied. Rule derivation is the stage's own, so the row text matches
//! what `backlot approve --scope` stores.

use backlot_stage::permit_rules::{AttentionTarget, Derivation, attention_target};

use crate::studio::StudioKey;

#[derive(Debug, Clone)]
pub(crate) enum ApprovalTarget {
    Studio { key: StudioKey, role: String },
    Hollywood,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ApprovalChoice {
    Once,
    Role,
    Studio,
    Deny,
}

#[derive(Debug, Clone)]
pub(crate) struct ApprovalDialog {
    pub(crate) title: String,
    pub(crate) target: ApprovalTarget,
    pub(crate) role_label: String,
    pub(crate) detail: String,
    pub(crate) selected: usize,
    pub(crate) rules: Vec<String>,
    pub(crate) outward: bool,
    pub(crate) once_only: Option<String>,
}

impl ApprovalDialog {
    pub(crate) fn new(
        target: ApprovalTarget,
        role_label: &str,
        detail: Option<&str>,
        select_deny: bool,
    ) -> Self {
        let detail = detail.unwrap_or("unbound prompt — peek screen").to_string();
        let (rules, outward, once_only) = match attention_target(&detail) {
            AttentionTarget::Edit(path) => (vec![path], false, None),
            AttentionTarget::Shell(Derivation::Rules(rs)) => (
                rs.iter().map(|r| r.rule.clone()).collect(),
                rs.iter().any(|r| r.outward),
                None,
            ),
            AttentionTarget::Shell(Derivation::OnceOnly(why)) => (Vec::new(), false, Some(why)),
            AttentionTarget::Unknown => (
                Vec::new(),
                false,
                Some("the prompt command cannot be identified from its visible block".into()),
            ),
        };
        let title = match &target {
            ApprovalTarget::Studio { key, role } => format!(
                "Approve {role}? · {}",
                crate::studio_dialog::StudioDialog::place(key)
            ),
            ApprovalTarget::Hollywood => "Approve Hollywood?".to_string(),
        };
        Self {
            title,
            target,
            role_label: role_label.to_string(),
            detail,
            selected: if select_deny { 1 } else { 0 },
            rules,
            outward,
            once_only,
        }
    }

    pub(crate) fn rule_text(&self) -> String {
        if self.rules.is_empty() {
            "no rememberable rule".to_string()
        } else {
            self.rules.join(" + ")
        }
    }

    pub(crate) fn choices(&self) -> Vec<(ApprovalChoice, String)> {
        let mut out = vec![(ApprovalChoice::Once, "Allow once".to_string())];
        if self.once_only.is_none() {
            out.push((
                ApprovalChoice::Role,
                format!("Allow for {} — {}", self.role_label, self.rule_text()),
            ));
            // Hollywood has no wider studio scope; its choice list stops
            // at its own role. Outward commands are never studio-wide.
            if !self.outward && !matches!(self.target, ApprovalTarget::Hollywood) {
                out.push((
                    ApprovalChoice::Studio,
                    format!("Allow for the studio — {}", self.rule_text()),
                ));
            }
        }
        out.push((ApprovalChoice::Deny, "Deny".to_string()));
        out
    }

    pub(crate) fn chosen(&self) -> ApprovalChoice {
        self.choices()
            .get(self.selected)
            .map(|(c, _)| *c)
            .unwrap_or(ApprovalChoice::Once)
    }

    pub(crate) fn move_selection(&mut self, delta: isize) {
        let len = self.choices().len().max(1) as isize;
        self.selected = (self.selected as isize + delta).rem_euclid(len) as usize;
    }

    /// Move the initial selection to Deny when `d` opened the dialog.
    pub(crate) fn select_deny(&mut self) {
        if let Some(at) = self
            .choices()
            .iter()
            .position(|(c, _)| *c == ApprovalChoice::Deny)
        {
            self.selected = at;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_choice_list_matches_scope_rules() {
        let key = StudioKey::local("acme-shop");
        let d = ApprovalDialog::new(
            ApprovalTarget::Studio {
                key,
                role: "actor-alice".into(),
            },
            "alice",
            Some("git status"),
            false,
        );
        let labels: Vec<String> = d.choices().into_iter().map(|(_, l)| l).collect();
        assert_eq!(labels[0], "Allow once");
        assert!(labels[1].starts_with("Allow for alice — git"));
        assert!(labels[2].starts_with("Allow for the studio — git"));
        assert_eq!(labels.last().unwrap(), "Deny");

        let h = ApprovalDialog::new(
            ApprovalTarget::Hollywood,
            "hollywood",
            Some("git status"),
            false,
        );
        let kinds: Vec<ApprovalChoice> = h.choices().into_iter().map(|(c, _)| c).collect();
        assert_eq!(
            kinds,
            vec![
                ApprovalChoice::Once,
                ApprovalChoice::Role,
                ApprovalChoice::Deny
            ]
        );
    }

    #[test]
    fn outward_and_once_only_prompts_hide_wide_grants() {
        let key = StudioKey::local("acme-shop");
        let outward = ApprovalDialog::new(
            ApprovalTarget::Studio {
                key: key.clone(),
                role: "actor-alice".into(),
            },
            "alice",
            Some("git push origin main"),
            false,
        );
        let kinds: Vec<ApprovalChoice> = outward.choices().into_iter().map(|(c, _)| c).collect();
        assert!(!kinds.contains(&ApprovalChoice::Studio), "{kinds:?}");

        let once = ApprovalDialog::new(
            ApprovalTarget::Studio {
                key,
                role: "actor-alice".into(),
            },
            "alice",
            Some("rm -rf build"),
            false,
        );
        let kinds: Vec<ApprovalChoice> = once.choices().into_iter().map(|(c, _)| c).collect();
        assert_eq!(kinds, vec![ApprovalChoice::Once, ApprovalChoice::Deny]);
        assert!(once.once_only.is_some());
    }
}
