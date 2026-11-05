// Derived from herdr src/config/sidebar/rules.rs (Apache-2.0).
// Deserialize-only: hc-tui never writes the user config.

use serde::Deserialize;

use super::{SidebarTokenColor, SidebarTokenStyle};

#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(try_from = "RawRule")]
pub struct SidebarTokenRule {
    condition: Condition,
    ignore_case: bool,
    style: SidebarTokenStyle,
    hide: Option<bool>,
}

// Deserialization rejects non-finite thresholds, so equality is reflexive.
impl Eq for SidebarTokenRule {}

#[derive(Debug, Clone, PartialEq)]
enum Condition {
    Equals(String),
    Contains(String),
    StartsWith(String),
    GreaterThan(f64),
    LessThan(f64),
}

#[derive(Clone, Default, Deserialize)]
#[serde(default, deny_unknown_fields)]
struct RawRule {
    equals: Option<String>,
    contains: Option<String>,
    starts_with: Option<String>,
    gt: Option<f64>,
    lt: Option<f64>,
    ignore_case: Option<bool>,
    fg: Option<SidebarTokenColor>,
    bold: Option<bool>,
    dim: Option<bool>,
    hide: Option<bool>,
}

impl TryFrom<RawRule> for SidebarTokenRule {
    type Error = String;

    fn try_from(raw: RawRule) -> Result<Self, Self::Error> {
        let count = [
            raw.equals.is_some(),
            raw.contains.is_some(),
            raw.starts_with.is_some(),
            raw.gt.is_some(),
            raw.lt.is_some(),
        ]
        .into_iter()
        .filter(|present| *present)
        .count();
        if count != 1 {
            return Err(
                "sidebar rule requires exactly one of equals, contains, starts_with, gt, lt".into(),
            );
        }
        let condition = if let Some(value) = raw.equals {
            Condition::Equals(value)
        } else if let Some(value) = raw.contains {
            Condition::Contains(value)
        } else if let Some(value) = raw.starts_with {
            Condition::StartsWith(value)
        } else {
            let (value, greater) = match (raw.gt, raw.lt) {
                (Some(value), _) => (value, true),
                (_, Some(value)) => (value, false),
                _ => unreachable!("validated condition count"),
            };
            if !value.is_finite() {
                return Err("sidebar numeric rule threshold must be finite".into());
            }
            if raw.ignore_case.is_some() {
                return Err("ignore_case applies only to sidebar text conditions".into());
            }
            if greater {
                Condition::GreaterThan(value)
            } else {
                Condition::LessThan(value)
            }
        };
        Ok(Self {
            condition,
            ignore_case: raw.ignore_case.unwrap_or(false),
            hide: raw.hide,
            style: SidebarTokenStyle {
                fg: raw.fg,
                bold: raw.bold,
                dim: raw.dim,
            },
        })
    }
}

impl SidebarTokenRule {
    fn matches(&self, value: &str, numeric: &mut Option<Option<f64>>) -> bool {
        match &self.condition {
            Condition::Equals(expected) => {
                if self.ignore_case {
                    value.eq_ignore_ascii_case(expected)
                } else {
                    value == expected
                }
            }
            Condition::StartsWith(expected) => {
                if self.ignore_case {
                    value
                        .as_bytes()
                        .get(..expected.len())
                        .is_some_and(|prefix| prefix.eq_ignore_ascii_case(expected.as_bytes()))
                } else {
                    value.starts_with(expected)
                }
            }
            Condition::Contains(expected) => {
                if self.ignore_case {
                    expected.is_empty()
                        || value
                            .as_bytes()
                            .windows(expected.len())
                            .any(|part| part.eq_ignore_ascii_case(expected.as_bytes()))
                } else {
                    value.contains(expected)
                }
            }
            Condition::GreaterThan(threshold) | Condition::LessThan(threshold) => {
                let parsed = numeric.get_or_insert_with(|| {
                    value
                        .parse::<f64>()
                        .ok()
                        .filter(|number| number.is_finite())
                });
                parsed.is_some_and(|number| match self.condition {
                    Condition::GreaterThan(_) => number > *threshold,
                    _ => number < *threshold,
                })
            }
        }
    }
}

pub(crate) fn matching_style(
    rules: &[SidebarTokenRule],
    base: SidebarTokenStyle,
    value: &str,
) -> Option<SidebarTokenStyle> {
    let mut numeric = None;
    for rule in rules {
        if rule.matches(value, &mut numeric) {
            if rule.hide == Some(true) {
                return None;
            }
            return Some(SidebarTokenStyle {
                fg: rule.style.fg.or(base.fg),
                bold: rule.style.bold.or(base.bold),
                dim: rule.style.dim.or(base.dim),
            });
        }
    }
    Some(base)
}
