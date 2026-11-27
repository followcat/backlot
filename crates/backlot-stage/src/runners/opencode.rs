//! OpenCode: `-s` chooses the session id at launch — it must start
//! with `ses` — so `role.started` carries it. The interactive command
//! has no model flag: the model goes through `OPENCODE_CONFIG_CONTENT`
//! with `--standalone`, which the background service needs to see it.
//! Effort is not applied either — `#variant` exists only on
//! `opencode run` and the adapter does not guess at it.

use std::collections::BTreeMap;

use backlot_model::ActorIdentity;

use super::{Launch, LaunchCtx, PlanError, Session, random_16};

/// `ses_` plus 16 random bytes as hex.
fn new_session_id() -> String {
    let mut id = String::from("ses_");
    for b in random_16() {
        id.push_str(&format!("{b:02x}"));
    }
    id
}

/// The `provider/model` ids `opencode models` printed for the
/// measured account's configured providers — the whole pair is
/// what `OPENCODE_CONFIG_CONTENT`'s `model` takes. Another
/// provider's id still goes in as free text.
pub const MODELS: &[&str] = &[
    "cline-pass/deepseek-v4.1-flash-1017",
    "cline-pass/deepseek-v4.1-flash",
    "cline-pass/deepseek-v4-pro",
    "cline-pass/glm-5.3",
    "cline-pass/kimi-k3.1-pro",
    "cline-pass/qwen3.8-max",
    "cline-pass/mimo-v2.6-pro",
    "cline-pass/minimax-m3",
    "opencode-go/claude-haiku-5-5",
    "opencode-go/deepseek-v4.1-flash",
    "opencode-go/gemini-4-argon",
    "opencode-go/glm-5.3",
    "opencode-go/grok-4.7",
    "opencode-go/hy4",
    "opencode-go/kimi-k3.1-pro",
    "opencode-go/qwen4-max",
    "opencode-go/mimo-v2.6-pro",
    "opencode/big-pickle",
    "opencode/deepseek-v4.1-flash-free",
    "longcat/LongCat-2.5-Preview",
    "ollama/qwen2.5:0.5b",
];

pub fn plan(identity: &ActorIdentity, ctx: &LaunchCtx) -> Result<Launch, PlanError> {
    let readonly = match identity.permission_profile.as_str() {
        "standard" | "worktree" => false,
        "readonly" => true,
        other => return Err(PlanError::profile(other)),
    };
    let session = ctx
        .resume
        .map(str::to_string)
        .unwrap_or_else(new_session_id);
    let mut argv = vec![
        "opencode".to_string(),
        "--standalone".into(),
        "-s".into(),
        session.clone(),
    ];
    if let Some(p) = ctx.prompt {
        argv.extend(["--prompt".into(), p.to_string()]);
    }
    let mut env = BTreeMap::new();
    let cfg = if readonly {
        serde_json::json!({"model": identity.model, "permission": {"edit": "deny"}})
    } else {
        serde_json::json!({"model": identity.model})
    };
    env.insert("OPENCODE_CONFIG_CONTENT".into(), cfg.to_string());
    Ok(Launch {
        argv,
        env,
        session: Session::Chosen(session),
        hooks: None,
        prep: None,
    })
}
