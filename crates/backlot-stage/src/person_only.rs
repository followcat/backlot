//! Code-derived list of person-only commands, flags, and outward actions
//! rendered into the producer, director, and writer prompts.

#[cfg(test)]
use crate::proc_guard::CliVerb;

/// Shared rule included in prompts ahead of the derived list.
pub(crate) const PERSON_ONLY_RULE: &str =
    "prepare the file, a dry-run command or the card request text, and tell the person";

/// Source guard categories behind the prompt's person-only actions list.
#[cfg_attr(not(test), allow(dead_code))]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum GuardKind {
    CardMutate,
    PermsRevoke,
    ServerControl,
    HollywoodControl,
    DesignSign,
    StudioCheck,
    RepoNoteAdmin,
    Approve,
    StudioCastAndRecast,
    StudioLifecycle,
    AskAcknowledge,
    ReviewMissingChecks,
    OutwardCardAndRefused,
}

#[cfg(test)]
impl GuardKind {
    /// Exhaustive mapping from every `CliVerb` gate to its prompt entry so
    /// adding a new CLI gate forces updating `ENTRIES` too.
    pub(crate) const fn from_cli_verb(verb: CliVerb<'_>) -> Self {
        match verb {
            CliVerb::CardMutate => Self::CardMutate,
            CliVerb::PermsRevoke => Self::PermsRevoke,
            CliVerb::ServerControl => Self::ServerControl,
            CliVerb::HollywoodControl => Self::HollywoodControl,
            CliVerb::DesignSign => Self::DesignSign,
            CliVerb::StudioCheck => Self::StudioCheck,
            CliVerb::RepoNoteAdmin => Self::RepoNoteAdmin,
            CliVerb::Approve { .. } => Self::Approve,
        }
    }
}

/// One bullet in the rendered `## Person-only actions` prompt section.
pub(crate) struct PersonOnlyEntry {
    #[cfg_attr(not(test), allow(dead_code))]
    pub kind: GuardKind,
    pub bullet: &'static str,
}

/// Derived from `proc_guard::CliVerb`, `roles.rs`, `commands.rs`, `verbs.rs`,
/// `verbs_design.rs`, `verbs_hwood.rs`, `hwood.rs`, `hwood_answers.rs`,
/// `hsetup.rs`, `hswitch_verbs.rs`, `delete.rs`, `restore.rs`, `studio_check.rs`,
/// `validate.rs`, `guard_git_push.rs`, and `guard_rules.rs`.
pub(crate) const ENTRIES: &[PersonOnlyEntry] = &[
    PersonOnlyEntry {
        kind: GuardKind::CardMutate,
        bullet: "Apply or cancel cards (`backlot card apply c<n>`, `backlot card cancel c<n>`).",
    },
    PersonOnlyEntry {
        kind: GuardKind::StudioCastAndRecast,
        bullet: "Change the studio cast, recast any role or actor, or restart Hollywood (`backlot studio cast set --file <file>` without `--dry-run`, `backlot recast --person <role> --runner <r> --model <m> --reason <r>`, `backlot restart --person hollywood --reason <r>`).",
    },
    PersonOnlyEntry {
        kind: GuardKind::StudioCheck,
        bullet: "Add or remove studio checks (`backlot studio check add --name <n> --run <r>`, `backlot studio check remove --name <n>`).",
    },
    PersonOnlyEntry {
        kind: GuardKind::DesignSign,
        bullet: "Sign off or skip opening design (`backlot design sign`, including `--skip` and `--no-checks`).",
    },
    PersonOnlyEntry {
        kind: GuardKind::StudioLifecycle,
        bullet: "Finish, reopen, delete, or restore a studio (`backlot studio finish --reason <r>`, `backlot studio reopen --reason <r>`, `backlot studio delete`, `backlot studio restore <archived-name>`).",
    },
    PersonOnlyEntry {
        kind: GuardKind::HollywoodControl,
        bullet: "Configure, switch, or cancel Hollywood setup (`backlot hollywood setup --runner <r> --model <m>`, `backlot hollywood switch --status` or `--cancel`).",
    },
    PersonOnlyEntry {
        kind: GuardKind::Approve,
        bullet: "Answer non-actor, self, or user-owned permission prompts (`backlot approve --person <role> --decision allow`, or `deny`, for `producer`, `director`, `writer`, `hollywood`, self, or `needs_user` prompts).",
    },
    PersonOnlyEntry {
        kind: GuardKind::ReviewMissingChecks,
        bullet: "Accept a delivery while declared `[[checks]]` still have no recorded run (`backlot review --task <task> --delivery 1 --verdict accepted --conclusion <r>`).",
    },
    PersonOnlyEntry {
        kind: GuardKind::PermsRevoke,
        bullet: "List or revoke remembered permissions (`backlot permit list`, `backlot permit revoke --id <id>`).",
    },
    PersonOnlyEntry {
        kind: GuardKind::ServerControl,
        bullet: "Start, stop, adopt, or manage the server (`backlot server start`, `run`, `stop`, `adopt`, `service`).",
    },
    PersonOnlyEntry {
        kind: GuardKind::RepoNoteAdmin,
        bullet: "Show or remove approved repository notes directly (`backlot repo-note show`, `backlot repo-note remove --id <n>`; propose notes with `backlot repo-note propose --id <n> --text <r> --why <r>`).",
    },
    PersonOnlyEntry {
        kind: GuardKind::AskAcknowledge,
        bullet: "Acknowledge a user ask (`backlot ask acknowledged --person <role> --target <seq>`).",
    },
    PersonOnlyEntry {
        kind: GuardKind::OutwardCardAndRefused,
        bullet: "Protected-branch, default-branch, tag, unrecorded/new-remote, `--all`/`--branches`/glob/matching/detached git pushes and `gh pr create` (prepare a user card with `backlot ask --run --why <r> -- git push`), plus hard-refused outward commands (force/delete/mirror pushes, mutating `gh`, `hc` control, sockets/daemons, remote `ssh`/`scp`/`rsync`, `npm publish`/`cargo publish`).",
    },
];

/// Render the Markdown section spliced into `producer.md`, `director.md`, and `writer.md`.
///
/// When `design_gate` is false, the studio does not have opening-design turns, so
/// `DesignSign` is omitted to avoid leaking `backlot design` commands into non-gate prompts.
pub(crate) fn rendered_section(design_gate: bool) -> String {
    let mut out = format!(
        "## Person-only actions\n\n\
         These actions refuse every non-person caller — never try to work around\n\
         them or stall waiting on them; {PERSON_ONLY_RULE}:\n\n"
    );
    for entry in ENTRIES {
        if !design_gate && entry.kind == GuardKind::DesignSign {
            continue;
        }
        out.push_str("- ");
        out.push_str(entry.bullet);
        out.push('\n');
    }
    out.trim_end().to_string()
}
