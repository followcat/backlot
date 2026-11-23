//! `docs/studio/runners.md`'s generated tables, rendered from the
//! runner caps. The docs test regenerates each span between its
//! markers and fails when the file has drifted; `BACKLOT_UPDATE_DOCS=1`
//! rewrites the file in place.

use backlot_model::caps::{
    self, Discovery, Effort, FirstPrompt, FolderTrust, Model, PromptDetection, Resume, StatusSource,
};

pub(crate) const CAPS_BEGIN: &str = "<!-- BEGIN GENERATED runner-caps -->";
pub(crate) const CAPS_END: &str = "<!-- END GENERATED runner-caps -->";
pub(crate) const APPROVAL_BEGIN: &str = "<!-- BEGIN GENERATED approval-keys -->";
pub(crate) const APPROVAL_END: &str = "<!-- END GENERATED approval-keys -->";

fn effort_cell(effort: Effort) -> String {
    match effort {
        Effort::Required(levels) => format!("required ({})", levels.join(", ")),
        Effort::Optional([]) => "optional (ignored)".to_string(),
        Effort::Optional(levels) => format!("optional ({})", levels.join(", ")),
        Effort::None => "none".to_string(),
    }
}

fn model_cell(model: Model) -> String {
    match model {
        Model::Flag(flag) => format!("`{flag}`"),
        Model::ProviderFlag { provider, flag } => format!("`-P {provider} {flag}`"),
        Model::Env(var) => format!("`{var}`"),
    }
}

fn first_prompt_cell(prompt: FirstPrompt) -> String {
    match prompt {
        FirstPrompt::Positional => "positional".to_string(),
        FirstPrompt::Flag(flag) => format!("`{flag}`"),
        FirstPrompt::File(flag) => format!("`{flag}` (file)"),
        FirstPrompt::None => "not carried".to_string(),
    }
}

fn resume_cell(resume: Resume, keeps_prompt: bool) -> String {
    let prompt = if keeps_prompt {
        "keeps prompt"
    } else {
        "prompt after open"
    };
    match resume {
        Resume::Argv(flags) => format!("{} ({prompt})", flags.join(", ")),
        Resume::Flag(flag) => format!("`{flag}` ({prompt}; id not read from argv)"),
        Resume::Subcommand => format!("`resume <id>` ({prompt})"),
        Resume::None => "none".to_string(),
    }
}

fn discovery_cell(discovery: Discovery) -> String {
    match discovery {
        Discovery::Argv => "argv".to_string(),
        Discovery::Files => "files".to_string(),
        Discovery::Command(command) => format!("`{command}`"),
        Discovery::MapThenSqlite => "cwd map, then sqlite".to_string(),
        Discovery::TerminalName => "terminal name".to_string(),
        Discovery::None => "none".to_string(),
    }
}

fn trust_cell(trust: FolderTrust) -> String {
    match trust {
        FolderTrust::StageAnswers => "stage answers",
        FolderTrust::LaunchHandles => "launch handles",
        FolderTrust::NotApplicable => "not applicable",
        FolderTrust::Unmeasured => "unmeasured",
    }
    .to_string()
}

fn detection_cell(detection: PromptDetection) -> String {
    let mut parts = Vec::new();
    if detection.block {
        parts.push("screen block");
    }
    if detection.hook {
        parts.push("hook");
    }
    if detection.structured {
        parts.push("structured status");
    }
    if parts.is_empty() {
        "none".to_string()
    } else {
        parts.join(", ")
    }
}

fn key_cell(key: Option<&str>, absent: &str) -> String {
    key.map_or_else(|| absent.to_string(), |key| format!("`{key}`"))
}

fn status_cell(status: StatusSource) -> String {
    match status {
        StatusSource::Native => "native 7501",
        StatusSource::ForkInference => "fork inference",
        StatusSource::ScreenOnly => "screen only",
        StatusSource::None => "none",
    }
    .to_string()
}

fn hooks_cell(hooks: &[&str]) -> String {
    if hooks.is_empty() {
        "none".to_string()
    } else {
        hooks.join(", ")
    }
}

/// The capability table: one row per runner.
pub(crate) fn capabilities_table() -> String {
    let mut out = String::from(
        "| Runner | Effort | Model | First prompt | Resume | Session discovery | \
         Folder trust | Prompt detection | Allow | Deny | Status source | Hooks |\n\
         |---|---|---|---|---|---|---|---|---|---|---|---|\n",
    );
    for caps in caps::ALL {
        out.push_str(&format!(
            "| {} | {} | {} | {} | {} | {} | {} | {} | {} | {} | {} | {} |\n",
            caps.label.trim_matches('`'),
            effort_cell(caps.effort),
            model_cell(caps.model),
            first_prompt_cell(caps.first_prompt),
            resume_cell(caps.resume, caps.resume_keeps_prompt),
            discovery_cell(caps.discovery),
            trust_cell(caps.folder_trust),
            detection_cell(caps.prompt_detection),
            key_cell(caps.approval_allow, "none sent"),
            key_cell(caps.approval_deny, "none sent"),
            status_cell(caps.status_source),
            hooks_cell(caps.hooks),
        ));
    }
    out
}

/// The "Markers from" prose that rides beside each approval row.
fn approval_note(runner: &str) -> &'static str {
    match runner {
        "fake" => "The script itself — byte-verified in `real_stage_closed_loop`",
        "claude" => {
            "Both measured on 2.1.319 — `enter` takes the allow option; `esc` does not run the \
             command and waits for input"
        }
        "codex" => "From its `› 1.` picker — still to be measured",
        "devin" => "Observed on 3000.12.32 — `enter` takes the highlighted `Once`, `esc` cancels",
        "opencode" => "Not measured — the attention stays open for the user",
        "cursor" => {
            "`y` takes the highlighted allow option — shell measured on 2026.11.05, the file \
             picker's `(y)` from the S12e capture. A denied command is retried, so no deny key \
             is mapped and the attention stays open"
        }
        "cmd" => {
            "Both measured on 1.88.5 — `enter` takes `❯ 1. Yes` (run-once); `esc` or `3` ends \
             the turn. Option `2` is never sent — it writes a project allow rule"
        }
        "cline" => {
            "The block only appears when `--yolo`'s auto-approve was switched off by hand; both \
             keys are a mode the stage must not change (S14a)"
        }
        "agy" => "Unmeasured — the attention stays open for the user",
        _ => "—",
    }
}

/// The approval-keys table: `allow` and `deny` read from the caps,
/// with the measured note beside them.
pub(crate) fn approval_table() -> String {
    let mut out = String::from("| Runner | `allow` | `deny` | Markers from |\n|---|---|---|---|\n");
    for caps in caps::ALL {
        out.push_str(&format!(
            "| {} | {} | {} | {} |\n",
            caps.label.trim_matches('`'),
            key_cell(caps.approval_allow, "none sent"),
            key_cell(caps.approval_deny, "none sent"),
            approval_note(caps.name),
        ));
    }
    out
}

/// Replace the body between `begin` and `end`, markers kept. `body`
/// ends with a newline; the end marker starts its own line.
pub(crate) fn replace_generated(text: &str, begin: &str, end: &str, body: &str) -> Option<String> {
    let start = text.find(begin)? + begin.len();
    let stop = text[start..].find(end)? + start;
    let mut out = String::with_capacity(text.len() + body.len());
    out.push_str(&text[..start]);
    out.push('\n');
    out.push_str(body);
    out.push_str(&text[stop..]);
    Some(out)
}
