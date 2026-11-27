//! Role prompts (S5): one markdown file per role, compiled in and
//! rendered into `<studio dir>/prompts/<role>.md` on a fresh start.
//! The first prompt the runner gets is one line pointing at that
//! file — for `devin` the line itself goes to a file, as
//! `LaunchCtx.prompt` wants. A resume gets no first prompt.

use std::collections::BTreeMap;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use backlot_model::state::StudioState;

use crate::reconcile::{RoleKind, Start};
use crate::workareas;

const PRODUCER: &str = include_str!("../prompts/producer.md");
const DIRECTOR: &str = include_str!("../prompts/director.md");
const WRITER: &str = include_str!("../prompts/writer.md");
const ACTOR: &str = include_str!("../prompts/actor.md");
/// The section a fixed role's first prompt gains when `role.recast`
/// replaced the agent that used to play it.
const RECAST: &str = include_str!("../prompts/recast.md");

/// Hollywood's own prompt — written to `<base>/hollywood/AGENTS.md`
/// on first launch (H3). `{core}` is the core's name.
pub(crate) const HOLLYWOOD: &str = include_str!("../prompts/hollywood.md");

/// The template a start's role renders.
pub fn template(kind: RoleKind) -> &'static str {
    match kind {
        RoleKind::Producer => PRODUCER,
        RoleKind::Director => DIRECTOR,
        RoleKind::Writer => WRITER,
        RoleKind::Actor => ACTOR,
    }
}

/// Keep or strip `{#design_gate}...{/design_gate}` blocks before field
/// substitution so `design_gate = off` renders stay byte-identical to
/// templates without the section.
fn apply_design_gate_block(template: &str, required: bool) -> String {
    const OPEN: &str = "{#design_gate}";
    const CLOSE: &str = "{/design_gate}";
    if !template.contains(OPEN) {
        return template.to_string();
    }
    let mut out = String::with_capacity(template.len());
    let mut rest = template;
    while let Some(start) = rest.find(OPEN) {
        out.push_str(&rest[..start]);
        let after_open = &rest[start + OPEN.len()..];
        let Some(end) = after_open.find(CLOSE) else {
            out.push_str(OPEN);
            out.push_str(after_open);
            return out;
        };
        let body = &after_open[..end];
        let after_close = &after_open[end + CLOSE.len()..];
        if required {
            out.push_str(body.strip_prefix('\n').unwrap_or(body));
        }
        rest = after_close.strip_prefix('\n').unwrap_or(after_close);
    }
    out.push_str(rest);
    out
}

/// `{name}` → the field's value. An unknown placeholder stays
/// literal so a template typo is visible, not silently dropped.
pub fn render(template: &str, fields: &BTreeMap<String, String>) -> String {
    let required = fields.get("design_gate").map(String::as_str) == Some("required");
    let mut out = apply_design_gate_block(template, required);
    for (k, v) in fields {
        out = out.replace(&format!("{{{k}}}"), v);
    }
    out
}

/// Every placeholder a start's prompt fills: the studio, the role
/// and its task, the requirement file and version, the work folder
/// and branch the terminal starts in, and the checks block — the
/// declared checks by `name` and `run` that `--check <name>` names,
/// or the sentence saying the cast declares none.
fn fields(
    studio: &str,
    dir: &Path,
    start: &Start,
    state: &StudioState,
    work: &Path,
) -> BTreeMap<String, String> {
    let req = if state.requirement_version == 0 {
        String::new()
    } else {
        dir.join(format!("req/v{}.md", state.requirement_version))
            .to_string_lossy()
            .into_owned()
    };
    let design_gate = match state.studio.as_ref().map(|s| s.design_gate) {
        Some(backlot_model::design::DesignGate::Required) => "required",
        _ => "off",
    };
    let design_file = if state.requirement_version == 0 {
        dir.join("design").to_string_lossy().into_owned()
    } else {
        dir.join(format!("design/v{}.md", state.requirement_version))
            .to_string_lossy()
            .into_owned()
    };
    let (task, title, acceptance) = match &start.task {
        Some(id) => match state.tasks.get(id) {
            Some(t) => (
                id.clone(),
                t.title.clone(),
                dir.join(&t.acceptance).to_string_lossy().into_owned(),
            ),
            None => (id.clone(), String::new(), String::new()),
        },
        None => (String::new(), String::new(), String::new()),
    };
    let branch = match start.kind {
        _ if start.no_git => "(no git — the studio was bound without a repository)".into(),
        RoleKind::Director => format!("backlot/{studio}/integration"),
        RoleKind::Actor => format!("backlot/{studio}/{task}"),
        _ => String::new(),
    };
    // A workspace's `work/<task>` holds one worktree per member
    // repository — the prompt names them so the actor commits in the
    // member its change belongs to (D10).
    let work = match start.members.is_empty() {
        true => work.to_string_lossy().into_owned(),
        false => {
            let names: Vec<String> = start
                .members
                .iter()
                .filter_map(|m| std::path::Path::new(m).file_name())
                .map(|n| n.to_string_lossy().into_owned())
                .collect();
            format!(
                "{} — one worktree per member repository inside: {}",
                work.display(),
                names.join(", ")
            )
        }
    };
    let declared = state
        .studio
        .as_ref()
        .map(|s| s.checks.as_slice())
        .unwrap_or(&[]);
    let checks = if declared.is_empty() {
        concat!(
            "- `backlot deliver --commits <A..B> --report <file>` — the cast\n",
            "  declares no checks, so `deliver` takes no `--check` here.\n"
        )
        .to_string()
    } else {
        let mut block =
            "- The stage runs the cast's declared checks on your delivery itself:\n".to_string();
        for c in declared {
            block.push_str(&format!("  - `{}`: `{}`\n", c.name, c.run));
        }
        block.push_str(concat!(
            "- `backlot deliver --commits <A..B> --report <file> --check <name>=pass`\n",
            "  (a check that failed is `--check <name>=fail`) — `<name>` is one of\n",
            "  the checks above.\n"
        ));
        block
    };
    BTreeMap::from([
        ("studio".into(), studio.into()),
        ("studio_dir".into(), dir.to_string_lossy().into_owned()),
        ("role".into(), start.kind.name().into()),
        ("task".into(), task),
        ("title".into(), title),
        ("acceptance".into(), acceptance),
        ("req_file".into(), req),
        ("req_version".into(), state.requirement_version.to_string()),
        ("design_gate".into(), design_gate.into()),
        ("design_file".into(), design_file),
        ("work".into(), work),
        ("branch".into(), branch),
        ("checks".into(), checks),
        (
            "actor_cap".into(),
            state
                .studio
                .as_ref()
                .map_or_else(backlot_model::event::default_max_actors, |s| s.max_actors)
                .to_string(),
        ),
    ])
}

/// R14: the appendix an experimental studio adds to each role's
/// prompt — research-level process guidance on top of the shared
/// one. A standard studio appends nothing, so its rendered prompts
/// stay byte-identical.
fn mode_guidance(kind: RoleKind, state: &StudioState) -> &'static str {
    use backlot_model::event::StudioMode;
    if state.studio.as_ref().map(|s| s.mode) != Some(StudioMode::Experimental) {
        return "";
    }
    match kind {
        RoleKind::Producer => concat!(
            "\n## Experimental production\n",
            "\n",
            "This studio runs in `experimental` mode: the product is\n",
            "knowledge, not only a shipped change. Before the work\n",
            "starts, write the research questions and hypotheses down,\n",
            "the criteria that will judge them, and the conditions\n",
            "that kill or pivot the line. Prefer a comparison of\n",
            "competing approaches over a single guess when the\n",
            "question allows it. A delivery that disproves a\n",
            "hypothesis is a result: record it honestly in the\n",
            "requirement notes rather than forcing a ship.\n",
        ),
        RoleKind::Director => concat!(
            "\n## Experimental production\n",
            "\n",
            "This studio runs in `experimental` mode. Where the\n",
            "question allows it, split competing approaches into\n",
            "parallel tasks and cast them across candidates —\n",
            "`backlot cast --task <task> --suggest` shows each\n",
            "candidate's factual record for the task's kind first.\n",
            "Review each delivery against the hypothesis and its\n",
            "evidence: a well-documented negative finding that rules\n",
            "out an approach is a valid `accepted` outcome. In node\n",
            "reports, compare the variants and state which evidence\n",
            "decided — including hypotheses that did not hold.\n",
        ),
        RoleKind::Writer => concat!(
            "\n## Experimental production\n",
            "\n",
            "This studio runs in `experimental` mode. In plans and\n",
            "acceptance lists, state the hypothesis being tested, the\n",
            "competing designs or baselines it is compared against,\n",
            "the measurable criteria that decide between them, and\n",
            "the conditions that would falsify the hypothesis. Write\n",
            "criteria a delivery can actually be checked against —\n",
            "numbers, outputs, behaviors — not aspirations.\n",
        ),
        RoleKind::Actor => concat!(
            "\n## Experimental production\n",
            "\n",
            "This studio runs in `experimental` mode. Treat the task\n",
            "as a controlled experiment: isolate the variant on your\n",
            "task branch, keep the baseline it is compared against\n",
            "untouched, and record the exact commands, measurements\n",
            "and caveats in your delivery report so the result\n",
            "reproduces. Never fabricate a metric or soften a failed\n",
            "hypothesis — a clean negative result is a deliverable.\n",
        ),
    }
}

/// Render the role's prompt to `<dir>/prompts/<role>.md` and return
/// the path — the work folder is the area `ensure` made (or will).
pub fn prepare(
    dir: &Path,
    studio: &str,
    start: &Start,
    state: &StudioState,
) -> io::Result<PathBuf> {
    let prompts = dir.join("prompts");
    fs::create_dir_all(&prompts)?;
    let work = workareas::role_dir(dir, &start.repo, &start.role, start.no_git);
    let fields = fields(studio, dir, start, state, &work);
    let mut text = render(template(start.kind), &fields);
    // A role whose previous agent ran a different identity is taking
    // the role over: it must read the state it inherits before acting.
    // An actor's identity moved if its task's newest cast was rewritten
    // by a `role.recast` of the actor key.
    let recast = state.recasts.iter().any(|r| r.role == start.role)
        && state
            .roles
            .get(&start.role)
            .and_then(|r| r.actor.as_ref())
            .is_some_and(|old| !backlot_model::recast::same_identity(old, &start.actor));
    if recast {
        text.push_str(&render(RECAST, &fields));
    }
    text.push_str(mode_guidance(start.kind, state));
    if matches!(start.kind, RoleKind::Director | RoleKind::Writer)
        && let Some(notes_path) =
            crate::repo_note::existing_notes_path_for_studio_dir(dir, &start.repo)
    {
        text.push_str(&format!(
            "\n## Repository notes (read-only)\n\nRead-only operator notes for `{}` live at `{}`.\nRead them at session start; never copy credentials or edit this file directly (propose updates with `backlot repo-note propose`).\n",
            start.repo,
            notes_path.display()
        ));
    }
    let file = prompts.join(format!("{}.md", start.role));
    fs::write(&file, text)?;
    Ok(file)
}

/// The one-line first prompt a fresh start gets.
pub fn first_line(file: &Path) -> String {
    format!("Read {} and follow it.", file.display())
}

/// `devin` takes the first prompt as a file (`--prompt-file`): the
/// line written to `<dir>/prompts/<role>.first.txt`.
pub fn first_file(dir: &Path, role: &str, line: &str) -> io::Result<PathBuf> {
    let prompts = dir.join("prompts");
    fs::create_dir_all(&prompts)?;
    let file = prompts.join(format!("{role}.first.txt"));
    fs::write(&file, format!("{line}\n"))?;
    Ok(file)
}
