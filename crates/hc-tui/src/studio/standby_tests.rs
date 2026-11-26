//! Standby role glyph rendering and frozen v0.4.0 client logic check.

use backlot_model::attention::AttentionClass;
use backlot_model::event::PERMISSION_PROMPT;
use backlot_model::summary::{RoleSummary, Work};

use super::glyphs::Person;
use crate::chrome::palette::Palette;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum V040Person {
    Working,
    Idle,
    Ready,
    Slot,
    UserPrompt,
    DirPrompt,
    NeedsYou,
    Asks,
    Stuck,
    Failed,
    Lost,
    Resumable,
    Closed,
}

impl V040Person {
    /// Frozen `v0.4.0` `Person::of` from `crates/hc-tui/src/studio/glyphs.rs` at tag `v0.4.0`.
    fn of(role: &RoleSummary) -> V040Person {
        match role.state.as_str() {
            "closed" => {
                return if role.resumable {
                    V040Person::Resumable
                } else {
                    V040Person::Closed
                };
            }
            "lost" => return V040Person::Lost,
            "waiting" => return V040Person::Slot,
            _ => {}
        }
        if let Some(attention) = role.attention.as_deref() {
            let class = role
                .attention_class
                .unwrap_or_else(|| AttentionClass::of(attention, role.attention_user));
            match class {
                AttentionClass::Info => {}
                AttentionClass::NeedsYou => {
                    let user_marked = role.attention_user || attention.ends_with("needs the user");
                    return if attention.starts_with(PERMISSION_PROMPT) {
                        let non_actor = role.task.is_none() && !role.role.starts_with("actor-");
                        if user_marked || non_actor {
                            V040Person::UserPrompt
                        } else {
                            V040Person::DirPrompt
                        }
                    } else if user_marked {
                        V040Person::UserPrompt
                    } else {
                        V040Person::NeedsYou
                    };
                }
                AttentionClass::Stuck => return V040Person::Stuck,
            }
        }
        if role.asks.is_some() {
            return V040Person::Asks;
        }
        if role.failed.is_some() {
            return V040Person::Failed;
        }
        match role.work {
            Some(Work::Working) => V040Person::Working,
            Some(Work::Blocked) => {
                if role.task.is_some() || role.role.starts_with("actor-") {
                    V040Person::DirPrompt
                } else {
                    V040Person::UserPrompt
                }
            }
            _ if role.task.is_none() && role.doing.as_deref().is_none_or(|d| d == "idle") => {
                V040Person::Ready
            }
            _ => V040Person::Idle,
        }
    }

    fn is_needs_you(self) -> bool {
        matches!(
            self,
            V040Person::UserPrompt
                | V040Person::DirPrompt
                | V040Person::NeedsYou
                | V040Person::Asks
                | V040Person::Stuck
                | V040Person::Failed
                | V040Person::Lost
        )
    }
}

fn standby_writer() -> RoleSummary {
    serde_json::from_value(serde_json::json!({
        "role": "writer",
        "name": "writer",
        "runner": "claude",
        "model": "claude-opus-4-7",
        "effort": "high",
        "state": "standby",
        "can_answer_prompt": true
    }))
    .unwrap()
}

#[test]
fn v040_client_logic_classifies_standby_without_needs_you_mark_or_working() {
    let writer = standby_writer();
    let v040 = V040Person::of(&writer);
    assert_eq!(v040, V040Person::Ready);
    assert!(!v040.is_needs_you());
    assert!(!writer.attention_user);
    assert_ne!(v040, V040Person::Working);
    assert_eq!(super::live_terminal(&writer), None);
}

#[test]
fn standby_mark_is_dim_blank_without_activity_or_attention() {
    let palette = Palette::from_name("catppuccin").unwrap();
    let writer = standby_writer();
    let mark = Person::of(&writer);
    assert_eq!(mark.glyph(0), " ");
    assert_eq!(mark.still(), " ");
    assert_eq!(mark.color(&palette), palette.overlay0);
    assert_eq!(mark.word(), "standby");
    assert_eq!(Person::inline_state(&writer, 1).word, "standby");
    assert_eq!(Person::inline_state(&writer, 1).detail, None);
}
