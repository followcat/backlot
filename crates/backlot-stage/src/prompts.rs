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

/// The template a start's role renders.
pub fn template(kind: RoleKind) -> &'static str {
    match kind {
        RoleKind::Producer => PRODUCER,
        RoleKind::Director => DIRECTOR,
        RoleKind::Writer => WRITER,
        RoleKind::Actor => ACTOR,
    }
}

/// `{name}` → the field's value. An unknown placeholder stays
/// literal so a template typo is visible, not silently dropped.
pub fn render(template: &str, fields: &BTreeMap<String, String>) -> String {
    let mut out = template.to_string();
    for (k, v) in fields {
        out = out.replace(&format!("{{{k}}}"), v);
    }
    out
}

/// Every placeholder a start's prompt fills: the studio, the role
/// and its task, the requirement file and version, and the work
/// folder and branch the terminal starts in.
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
        RoleKind::Director => format!("backlot/{studio}/integration"),
        RoleKind::Actor => format!("backlot/{studio}/{task}"),
        _ => String::new(),
    };
    BTreeMap::from([
        ("studio".into(), studio.into()),
        ("role".into(), start.kind.name().into()),
        ("task".into(), task),
        ("title".into(), title),
        ("acceptance".into(), acceptance),
        ("req_file".into(), req),
        ("req_version".into(), state.requirement_version.to_string()),
        ("work".into(), work.to_string_lossy().into_owned()),
        ("branch".into(), branch),
    ])
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
    let work = workareas::role_dir(dir, &start.repo, &start.role);
    let text = render(
        template(start.kind),
        &fields(studio, dir, start, state, &work),
    );
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
    let file = dir.join("prompts").join(format!("{role}.first.txt"));
    fs::write(&file, format!("{line}\n"))?;
    Ok(file)
}
