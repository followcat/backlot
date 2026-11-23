//! Runner capabilities as data: what each adapter can do, stated once.
//! The stage reads a capability instead of re-deriving it from a
//! `match` arm, the summary publishes the one fact the client needs
//! ([`RunnerCaps::can_answer`]), and `docs/studio/runners.md`'s
//! generated tables are rendered from [`ALL`].
//!
//! Every field states the measured fact the code and `runners.md`
//! carry today — nothing here is aspirational.

/// How a runner takes its thinking depth.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Effort {
    /// A cast must declare one of these levels.
    Required(&'static [&'static str]),
    /// A cast may declare one; the adapter instead bakes the depth into
    /// the model id and emits no flag.
    Optional(&'static [&'static str]),
    /// No effort setting: a declared level is refused (it would lie in
    /// the ledger) and no flag is emitted.
    None,
}

impl Effort {
    /// The accepted levels — empty where the runner has none.
    pub fn levels(self) -> &'static [&'static str] {
        match self {
            Effort::Required(levels) | Effort::Optional(levels) => levels,
            Effort::None => &[],
        }
    }
}

/// Why an effort does not fit a runner.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EffortProblem {
    /// The runner takes an effort and none was declared.
    Missing,
    /// The runner has no effort setting and one was declared.
    NotAllowed,
}

/// The refusal line for an effort that does not fit — the recast,
/// cast and Hollywood config errors all read it.
pub fn effort_message(runner: &str, problem: EffortProblem) -> String {
    match problem {
        EffortProblem::Missing => format!("runner '{runner}' needs an effort"),
        EffortProblem::NotAllowed => {
            format!("runner '{runner}' takes no effort — the depth is part of the model id")
        }
    }
}

/// The cast file's effort rule: a `Required` runner needs a level, a
/// `None` runner refuses one, and an `Optional` runner accepts either.
/// A runner with no cap row (`runner-w` in a hand-written cast, before
/// the supported-runner check runs) keeps the old rule: it needs a
/// level.
pub fn check_effort(runner: &str, effort: Option<&str>) -> Result<(), EffortProblem> {
    let effort = effort.filter(|e| !e.is_empty());
    match get(runner) {
        Some(caps) => match caps.effort {
            Effort::Required(_) if effort.is_none() => Err(EffortProblem::Missing),
            Effort::None if effort.is_some() => Err(EffortProblem::NotAllowed),
            _ => Ok(()),
        },
        None if effort.is_none() => Err(EffortProblem::Missing),
        None => Ok(()),
    }
}

/// How the model id reaches the runner.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Model {
    /// A flag and its value.
    Flag(&'static str),
    /// A provider flag beside the model flag (`-P cline-pass -m <id>`).
    ProviderFlag {
        provider: &'static str,
        flag: &'static str,
    },
    /// An environment variable (`FAKE_MODEL`, OpenCode's JSON config).
    Env(&'static str),
}

/// How a fresh, interactive launch carries the first prompt.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FirstPrompt {
    /// A trailing positional argument.
    Positional,
    /// A flag carrying the text.
    Flag(&'static str),
    /// A file the caller wrote.
    File(&'static str),
    /// The adapter carries none.
    None,
}

/// Resume support, and whether the id can be read back from a live
/// terminal's argv.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Resume {
    /// Resume by id through one of these flags; the stage reads the id
    /// back from the terminal's argv.
    Argv(&'static [&'static str]),
    /// Resume by id through a flag the stage does not read back — the
    /// id came from launch-time discovery instead.
    Flag(&'static str),
    /// `codex resume <id>`.
    Subcommand,
    /// The adapter does not resume.
    None,
}

/// Where a launch's session id becomes known.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Discovery {
    /// The terminal's argv carries it.
    Argv,
    /// A file or directory tree the runner wrote.
    Files,
    /// A runner command lists it.
    Command(&'static str),
    /// agy's cwd → id map, then a byte scan of its sqlite files.
    MapThenSqlite,
    /// The stage derives it from the terminal name (`fake`).
    TerminalName,
    /// Nothing discovers one.
    None,
}

/// The folder-trust dialog.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FolderTrust {
    /// The stage answers the dialog itself.
    StageAnswers,
    /// A launch flag or config keeps it away.
    LaunchHandles,
    /// The runner shows none.
    NotApplicable,
    /// Not measured.
    Unmeasured,
}

/// How a permission prompt is recognised.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PromptDetection {
    /// A saved prompt block can be compared before a key goes out.
    pub block: bool,
    /// A per-session hook reports it.
    pub hook: bool,
    /// The terminal's structured status reports `waiting_for_person`.
    pub structured: bool,
}

/// What fills the summary's work state and structured status.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StatusSource {
    /// The runner's own OSC 7501 records.
    Native,
    /// The fork core's screen inference manifests.
    ForkInference,
    /// The stage's own screen markers only.
    ScreenOnly,
    /// Nothing reports it.
    None,
}

/// One runner's capabilities, measured.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RunnerCaps {
    /// The runner name the ledger and CLI use.
    pub name: &'static str,
    /// The name the docs' prose tables use (`Claude Code`, `cursor-agent`).
    pub label: &'static str,
    pub effort: Effort,
    pub model: Model,
    pub first_prompt: FirstPrompt,
    /// Whether a resume still carries the first prompt; `false` means
    /// the stage sends it after the composer opens.
    pub resume_keeps_prompt: bool,
    pub resume: Resume,
    pub discovery: Discovery,
    pub folder_trust: FolderTrust,
    pub prompt_detection: PromptDetection,
    /// The key a recorded `allow` sends, if any.
    pub approval_allow: Option<&'static str>,
    /// The key a recorded `deny` sends, if any.
    pub approval_deny: Option<&'static str>,
    pub status_source: StatusSource,
    /// The hook events the per-session settings file registers.
    pub hooks: &'static [&'static str],
}

impl RunnerCaps {
    /// Whether a recorded decision can actually answer this runner's
    /// prompt — at least one of the two keys exists.
    pub fn can_answer(&self) -> bool {
        self.approval_allow.is_some() || self.approval_deny.is_some()
    }

    /// Whether the runner's session id is read back from a terminal's
    /// argv.
    pub fn session_in_argv(&self) -> bool {
        matches!(self.resume, Resume::Argv(_))
    }

    /// Whether the session id is discovered after launch.
    pub fn post_launch_session(&self) -> bool {
        matches!(
            self.discovery,
            Discovery::Files | Discovery::Command(_) | Discovery::MapThenSqlite
        )
    }
}

/// Whether `runner` can answer its own prompt; an unknown runner keeps
/// the old behaviour (documents written before this field read `true`).
pub fn can_answer_prompt(runner: &str) -> bool {
    get(runner).is_none_or(RunnerCaps::can_answer)
}

/// The capability row for `name`.
pub fn get(name: &str) -> Option<&'static RunnerCaps> {
    ALL.iter().copied().find(|c| c.name == name)
}

/// The hook events Claude Code's per-session settings file registers.
const CLAUDE_HOOKS: &[&str] = &[
    "UserPromptSubmit",
    "PreToolUse",
    "PostToolUse",
    "PostToolUseFailure",
    "PermissionRequest",
    "Notification",
    "Stop",
    "StopFailure",
];

pub static CLAUDE: RunnerCaps = RunnerCaps {
    name: "claude",
    label: "Claude Code",
    effort: Effort::Required(&["low", "medium", "high", "xhigh", "max"]),
    model: Model::Flag("--model"),
    first_prompt: FirstPrompt::Positional,
    resume_keeps_prompt: true,
    resume: Resume::Argv(&["--session-id", "-r"]),
    discovery: Discovery::Argv,
    folder_trust: FolderTrust::StageAnswers,
    prompt_detection: PromptDetection {
        block: true,
        hook: true,
        structured: true,
    },
    approval_allow: Some("enter"),
    approval_deny: Some("esc"),
    status_source: StatusSource::ForkInference,
    hooks: CLAUDE_HOOKS,
};

pub static CODEX: RunnerCaps = RunnerCaps {
    name: "codex",
    label: "Codex",
    effort: Effort::Required(&["low", "medium", "high", "xhigh", "max", "ultra"]),
    model: Model::Flag("-m"),
    first_prompt: FirstPrompt::Positional,
    resume_keeps_prompt: true,
    resume: Resume::Subcommand,
    discovery: Discovery::Files,
    folder_trust: FolderTrust::LaunchHandles,
    prompt_detection: PromptDetection {
        block: true,
        hook: false,
        structured: true,
    },
    approval_allow: Some("enter"),
    approval_deny: Some("esc"),
    status_source: StatusSource::ForkInference,
    hooks: &[],
};

pub static DEVIN: RunnerCaps = RunnerCaps {
    name: "devin",
    label: "Devin",
    effort: Effort::Optional(&[]),
    model: Model::Flag("--model"),
    first_prompt: FirstPrompt::File("--prompt-file"),
    resume_keeps_prompt: false,
    resume: Resume::Flag("-r"),
    discovery: Discovery::Command("devin list --format json"),
    folder_trust: FolderTrust::LaunchHandles,
    prompt_detection: PromptDetection {
        block: true,
        hook: false,
        structured: true,
    },
    approval_allow: Some("enter"),
    approval_deny: Some("esc"),
    status_source: StatusSource::Native,
    hooks: &[],
};

pub static OPENCODE: RunnerCaps = RunnerCaps {
    name: "opencode",
    label: "OpenCode",
    effort: Effort::Optional(&[]),
    model: Model::Env("OPENCODE_CONFIG_CONTENT"),
    first_prompt: FirstPrompt::Flag("--prompt"),
    resume_keeps_prompt: true,
    resume: Resume::Argv(&["-s"]),
    discovery: Discovery::Argv,
    folder_trust: FolderTrust::NotApplicable,
    prompt_detection: PromptDetection {
        block: false,
        hook: false,
        structured: true,
    },
    approval_allow: None,
    approval_deny: None,
    status_source: StatusSource::ForkInference,
    hooks: &[],
};

pub static CURSOR: RunnerCaps = RunnerCaps {
    name: "cursor",
    label: "cursor-agent",
    effort: Effort::None,
    model: Model::Flag("--model"),
    first_prompt: FirstPrompt::Positional,
    resume_keeps_prompt: true,
    resume: Resume::Flag("--resume"),
    discovery: Discovery::Files,
    folder_trust: FolderTrust::LaunchHandles,
    prompt_detection: PromptDetection {
        block: true,
        hook: false,
        structured: true,
    },
    approval_allow: Some("y"),
    approval_deny: None,
    status_source: StatusSource::ForkInference,
    hooks: &[],
};

pub static CMD: RunnerCaps = RunnerCaps {
    name: "cmd",
    label: "cmd",
    effort: Effort::None,
    model: Model::Flag("-m"),
    first_prompt: FirstPrompt::Positional,
    resume_keeps_prompt: false,
    resume: Resume::Argv(&["-r", "--resume", "--session"]),
    discovery: Discovery::Files,
    folder_trust: FolderTrust::LaunchHandles,
    prompt_detection: PromptDetection {
        block: true,
        hook: false,
        structured: true,
    },
    approval_allow: Some("enter"),
    approval_deny: Some("esc"),
    status_source: StatusSource::ForkInference,
    hooks: &[],
};

pub static CLINE: RunnerCaps = RunnerCaps {
    name: "cline",
    label: "cline",
    effort: Effort::Required(&["none", "low", "medium", "high", "xhigh"]),
    model: Model::ProviderFlag {
        provider: "cline-pass",
        flag: "-m",
    },
    first_prompt: FirstPrompt::Positional,
    resume_keeps_prompt: false,
    resume: Resume::Argv(&["--id"]),
    discovery: Discovery::Files,
    folder_trust: FolderTrust::NotApplicable,
    prompt_detection: PromptDetection {
        block: true,
        hook: false,
        structured: false,
    },
    approval_allow: None,
    approval_deny: None,
    status_source: StatusSource::ScreenOnly,
    hooks: &["PreToolUse"],
};

pub static AGY: RunnerCaps = RunnerCaps {
    name: "agy",
    label: "agy",
    effort: Effort::Required(&["low", "medium", "high", "xhigh", "max"]),
    model: Model::Flag("--model"),
    first_prompt: FirstPrompt::Flag("-i"),
    resume_keeps_prompt: false,
    resume: Resume::Argv(&["--conversation"]),
    discovery: Discovery::MapThenSqlite,
    folder_trust: FolderTrust::Unmeasured,
    prompt_detection: PromptDetection {
        block: false,
        hook: false,
        structured: false,
    },
    approval_allow: None,
    approval_deny: None,
    status_source: StatusSource::None,
    hooks: &[],
};

pub static FAKE: RunnerCaps = RunnerCaps {
    name: "fake",
    label: "`fake`",
    effort: Effort::Required(&["low", "medium", "high"]),
    model: Model::Env("FAKE_MODEL"),
    first_prompt: FirstPrompt::None,
    resume_keeps_prompt: false,
    resume: Resume::None,
    discovery: Discovery::TerminalName,
    folder_trust: FolderTrust::StageAnswers,
    prompt_detection: PromptDetection {
        block: true,
        hook: true,
        structured: false,
    },
    approval_allow: Some("enter"),
    approval_deny: Some("esc"),
    status_source: StatusSource::ScreenOnly,
    hooks: CLAUDE_HOOKS,
};

/// Every runner, in `runners.md`'s column order with `agy` and `fake`
/// last.
pub static ALL: &[&RunnerCaps] = &[
    &CLAUDE, &CODEX, &DEVIN, &OPENCODE, &CURSOR, &CMD, &CLINE, &AGY, &FAKE,
];

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_runner_has_one_cap_and_a_known_effort_rule() {
        for caps in ALL {
            assert_eq!(get(caps.name).map(|c| c.name), Some(caps.name));
            match caps.effort {
                Effort::Required(levels) => assert!(!levels.is_empty(), "{}", caps.name),
                Effort::Optional(levels) => assert!(levels.is_empty(), "{}", caps.name),
                Effort::None => {}
            }
        }
    }

    #[test]
    fn the_effort_rule_comes_from_the_caps() {
        assert_eq!(check_effort("claude", None), Err(EffortProblem::Missing));
        assert_eq!(
            check_effort("cursor", Some("high")),
            Err(EffortProblem::NotAllowed)
        );
        assert_eq!(check_effort("devin", Some("high")), Ok(()));
        assert_eq!(check_effort("cline", Some("high")), Ok(()));
    }

    #[test]
    fn can_answer_is_the_approval_keys() {
        assert!(!can_answer_prompt("opencode"));
        assert!(!can_answer_prompt("cline"));
        assert!(!can_answer_prompt("agy"));
        assert!(can_answer_prompt("cursor"), "an allow key alone answers");
        assert!(can_answer_prompt("cmd"));
        assert!(can_answer_prompt("claude"));
        assert!(
            can_answer_prompt("unknown"),
            "a missing cap keeps the old read"
        );
    }
}
