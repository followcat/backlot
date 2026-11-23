//! The cross-studio reads Hollywood and the user share: `card list`,
//! `status --all` and `records --all`/`--studio` (R24).

use std::collections::BTreeMap;
use std::path::Path;

use crate::args::{Cmd, Common};
use crate::cli::{Env, Fail};
use crate::commands;
use crate::config;
use crate::hrender;
use crate::hwood::{base_of, store_fail};
use crate::render;
use crate::store::{self, StoreError};

/// The read side of `cli::run`'s dispatch: the commands that never
/// append a studio event — `status`, `records` and `studio new
/// --dry-run` (D19, R24). `None` falls through to the append path.
pub fn read(common: &Common, cmd: &Cmd, env: &Env, base: &Path) -> Option<Result<String, Fail>> {
    match cmd {
        Cmd::Status { json, all: true } => Some(status_all(env, base, *json)),
        Cmd::Status { json, .. } => Some(status(common, env, base, *json)),
        Cmd::Records { json, all, kind } => {
            Some(records(common, env, base, *json, *all, kind.as_deref()))
        }
        Cmd::CastSuggest { json } => Some(cast_suggest(common, env, base, *json)),
        Cmd::ReportShow { file } => {
            let mut out = std::io::stdout().lock();
            Some(report_show(common, env, base, file, &mut out).map(|()| String::new()))
        }
        Cmd::StudioNew {
            dry_run: true,
            interactive: false,
            title,
            repo,
            cast_file,
            init,
            no_git,
            ask,
            ..
        } => Some(dry_run(
            common,
            env,
            base,
            &title.clone().unwrap_or_default(),
            &repo.clone().unwrap_or_default(),
            &cast_file.clone().unwrap_or_default(),
            (*init, *no_git),
            ask.as_ref(),
        )),
        _ => None,
    }
}

/// `status [--studio <name>]` for one studio.
fn status(common: &Common, env: &Env, base: &Path, json: bool) -> Result<String, Fail> {
    let name = crate::cli::studio_name(common, env)?;
    let dir = store::studio_dir(&crate::cli::resolve_base(env, common, base, &name)?, &name);
    let state = store::load_state(&dir).map_err(store_fail)?;
    Ok(if json {
        serde_json::to_string_pretty(&state).map_err(|e| Fail::refused(e.to_string()))?
    } else {
        let now_ms = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_millis() as u64)
            .unwrap_or(0);
        render::status(
            &name,
            &state,
            &crate::signals::failed_roles(&dir, &state),
            now_ms,
        )
    })
}

/// `records [--studio <name> | --all]`: `--studio` scopes to one;
/// plain and `--all` keep the every-studio read records has always
/// had.
fn records(
    common: &Common,
    env: &Env,
    base: &Path,
    json: bool,
    all: bool,
    kind: Option<&str>,
) -> Result<String, Fail> {
    let (names, base) = match (all, &common.studio) {
        (false, Some(name)) => {
            let name = crate::cli::check_studio_name(name)?;
            let base = crate::cli::resolve_base(env, common, base, &name)?;
            (vec![name], base)
        }
        _ => (
            store::studios(base).map_err(|e| Fail::refused(e.to_string()))?,
            base.to_path_buf(),
        ),
    };
    let mut logs = studio_logs(&base, &names)?;
    if all || common.studio.is_none() {
        logs.extend(archived_studio_logs(&base)?);
    }
    let mut recs = backlot_model::records(&logs);
    // `--kind <task_kind>` keeps only that kind's rows (L3).
    if let Some(kind) = kind {
        recs.retain(|r| r.task_kind == kind);
    }
    Ok(if json {
        serde_json::to_string_pretty(&recs).map_err(|e| Fail::refused(e.to_string()))?
    } else if recs.is_empty()
        && let Some(kind) = kind
    {
        format!("no deliveries recorded for kind '{kind}'\n")
    } else {
        render::records(&recs)
    })
}

/// Every archived studio's log under `<base>/archive/`, keyed by its
/// archive directory name (`<id>-<stamp>`) so archived history stays
/// distinct from any re-created studio with the same id.
fn archived_studio_logs(base: &Path) -> Result<Vec<backlot_model::StudioLog>, Fail> {
    let archived = store::archived_studios(base).map_err(|e| Fail::refused(e.to_string()))?;
    let mut logs = Vec::new();
    for (name, dir) in archived {
        let (events, _) = store::load_events(&dir).map_err(|e| {
            let detail = match e {
                StoreError::Io(e) => e.to_string(),
                StoreError::Corrupt(c) => c.to_string(),
                _ => "unexpected error".to_string(),
            };
            Fail::refused(format!("studio '{name}': {detail}"))
        })?;
        logs.push(backlot_model::StudioLog {
            studio: name,
            events,
        });
    }
    Ok(logs)
}

/// Every named studio's log for the cross-studio reads — the same
/// loader `records` and `cast --suggest` share.
fn studio_logs(base: &Path, names: &[String]) -> Result<Vec<backlot_model::StudioLog>, Fail> {
    let mut logs = Vec::new();
    for name in names {
        let (events, _) = store::load_events(&store::studio_dir(base, name)).map_err(|e| {
            let detail = match e {
                StoreError::Io(e) => e.to_string(),
                StoreError::Corrupt(c) => c.to_string(),
                _ => "unexpected error".to_string(),
            };
            Fail::refused(format!("studio '{name}': {detail}"))
        })?;
        logs.push(backlot_model::StudioLog {
            studio: name.clone(),
            events,
        });
    }
    Ok(logs)
}

/// `cast --task <id> --suggest` (L3, D8): each declared candidate's
/// factual record for the task's kind across every studio's log —
/// counts and honest notices only, never a score or a pick.
fn cast_suggest(common: &Common, env: &Env, base: &Path, json: bool) -> Result<String, Fail> {
    let name = crate::cli::studio_name(common, env)?;
    let task = common
        .task
        .clone()
        .or_else(|| env.get("BACKLOT_TASK").map(String::from))
        .ok_or_else(|| Fail::usage("missing task: set BACKLOT_TASK or pass --task"))?;
    let base = crate::cli::resolve_base(env, common, base, &name)?;
    let dir = store::studio_dir(&base, &name);
    let state = store::load_state(&dir).map_err(store_fail)?;
    let t = state
        .tasks
        .get(&task)
        .ok_or_else(|| Fail::refused(format!("no task '{task}'")))?;
    let candidates = state
        .studio
        .as_ref()
        .map(|s| s.candidates.as_slice())
        .unwrap_or(&[]);
    if candidates.is_empty() {
        return Err(Fail::refused(format!(
            "studio '{name}' declares no candidates"
        )));
    }
    let names = store::studios(&base).map_err(|e| Fail::refused(e.to_string()))?;
    let mut logs = studio_logs(&base, &names)?;
    logs.extend(archived_studio_logs(&base)?);
    let recs = backlot_model::records(&logs);
    let history = backlot_model::casting::candidate_history(candidates, &t.task_kind, &recs);
    Ok(if json {
        serde_json::to_string_pretty(&history).map_err(|e| Fail::refused(e.to_string()))?
    } else {
        render::cast_suggest(&task, &t.task_kind, &history)
    })
}

/// `report show --studio <s> --file <relpath>` (G3): streams one
/// studio artifact to `out`. Only `reports/…`, `checks/…`,
/// `messages/<file>`, `attention/<seq>.txt` and `requirements/v<n>.md`
/// (the on-disk `req/` directory — `req/v<n>.md` is accepted as the
/// same root) are served, the bytes are capped at 1 MiB, and the
/// resolved path must stay inside the studio directory — `..`,
/// absolute paths and symlinks that leave it are refused. `out` is a
/// parameter so the read can be captured in tests; production hands
/// it stdout.
pub(crate) fn report_show(
    common: &Common,
    env: &Env,
    base: &Path,
    file: &Path,
    out: &mut dyn std::io::Write,
) -> Result<(), Fail> {
    const MAX_BYTES: u64 = 1024 * 1024;
    let name = crate::cli::studio_name(common, env)?;
    let dir = store::studio_dir(&crate::cli::resolve_base(env, common, base, &name)?, &name);
    let rel = report_relpath(file)?;
    let root = dir
        .canonicalize()
        .map_err(|e| Fail::refused(e.to_string()))?;
    let real = dir
        .join(&rel)
        .canonicalize()
        .map_err(|_| Fail::refused(format!("no file '{}'", rel.display())))?;
    if !real.starts_with(&root) || !real.is_file() {
        return Err(Fail::refused(format!(
            "'{}' is outside studio '{name}'",
            rel.display()
        )));
    }
    let bytes = real
        .metadata()
        .map_err(|e| Fail::refused(e.to_string()))?
        .len();
    if bytes > MAX_BYTES {
        return Err(Fail::refused(format!(
            "'{}' is {} bytes — the report show limit is {} bytes",
            rel.display(),
            bytes,
            MAX_BYTES
        )));
    }
    let mut src = std::fs::File::open(&real).map_err(|e| Fail::refused(e.to_string()))?;
    std::io::copy(&mut src, out).map_err(|e| Fail::refused(e.to_string()))?;
    Ok(())
}

/// The `--file` argument as a validated studio-relative path — the
/// same check `report_show` runs before touching the filesystem.
pub(crate) fn report_relpath(file: &Path) -> Result<std::path::PathBuf, Fail> {
    use std::path::Component;
    if file.is_absolute()
        || file.as_os_str().is_empty()
        || file.as_os_str().to_string_lossy().contains('\0')
    {
        return Err(Fail::refused(format!(
            "--file '{}' must be a studio-relative path",
            file.display()
        )));
    }
    let parts: Vec<String> = file
        .components()
        .map(|c| match c {
            Component::Normal(p) => Ok(p.to_string_lossy().into_owned()),
            _ => Err(Fail::refused(format!(
                "--file '{}' must not contain '..'",
                file.display()
            ))),
        })
        .collect::<Result<_, _>>()?;
    let Some(root) = parts.first() else {
        return Err(Fail::refused("--file names no folder"));
    };
    let tail_ok =
        |parts: &[String], want: fn(&str) -> bool| parts.len() == 2 && want(parts[1].as_str());
    let name = parts.last().map(String::as_str).unwrap_or_default();
    let ok = match root.as_str() {
        "reports" | "checks" => parts.len() >= 2,
        "messages" => parts.len() == 2,
        "attention" => tail_ok(&parts, |n| {
            n.strip_suffix(".txt")
                .is_some_and(|seq| !seq.is_empty() && seq.chars().all(|c| c.is_ascii_digit()))
        }),
        // `requirements/` is the G3 name for the ledger's `req/`.
        "requirements" | "req" => tail_ok(&parts, |n| {
            n.strip_suffix(".md").is_some_and(|stem| {
                let ver = stem
                    .split_once('-')
                    .map_or(stem, |(ver, slug)| if !slug.is_empty() { ver } else { "" });
                ver.strip_prefix('v')
                    .is_some_and(|d| !d.is_empty() && d.chars().all(|c| c.is_ascii_digit()))
            })
        }),
        _ => false,
    };
    if !ok {
        return Err(Fail::refused(format!(
            "--file '{name}' is not under reports/, checks/, messages/, attention/ or requirements/"
        )));
    }
    let mut rel = std::path::PathBuf::from(root.as_str());
    if root == "requirements" {
        rel = std::path::PathBuf::from("req");
    }
    for part in &parts[1..] {
        rel.push(part);
    }
    Ok(rel)
}

/// `studio new --dry-run`: every `studio new` check plus the draft's
/// own (D19) — nothing recorded, nothing created. A `--ask`/`--ask-file`
/// is checked non-empty and restated in the plan, but not queued.
#[allow(clippy::too_many_arguments)]
fn dry_run(
    common: &Common,
    env: &Env,
    base: &Path,
    title: &str,
    repo: &str,
    cast_file: &Path,
    flags: (bool, bool),
    ask: Option<&crate::args::TextSource>,
) -> Result<String, Fail> {
    let name = crate::cli::studio_name(common, env)?;
    crate::draft::check(base, &name, repo, cast_file, env, flags.0, flags.1)?;
    let mut out =
        format!("dry run: studio '{name}' ('{title}', repo {repo}) is valid — nothing recorded");
    if let Some(ask) = ask {
        // The dry run precedes `materialize`, so `--ask`'s text is
        // still inline here; `--ask-file` names a file.
        let body = match ask {
            crate::args::TextSource::Text(text) => text.clone(),
            crate::args::TextSource::File(path) => std::fs::read_to_string(path)
                .map_err(|e| Fail::refused(format!("--ask-file {}: {e}", path.display())))?,
            crate::args::TextSource::Stdin => String::new(),
        };
        if body.trim().is_empty() {
            return Err(Fail::refused("the ask is empty"));
        }
        let line = body
            .lines()
            .find(|l| !l.trim().is_empty())
            .unwrap_or_default()
            .trim();
        let line: String = line.chars().take(60).collect();
        out.push_str(&format!("\nfirst message to the producer: {line}"));
    }
    Ok(out)
}

/// `card list` — every card Hollywood has proposed, open first.
pub fn card_list(common: &Common, env: &Env) -> Result<String, Fail> {
    let base = base_of(common, env)?;
    let hdir = store::hollywood_dir(&base);
    let hstate = store::load_hstate(&hdir).map_err(store_fail)?;
    let cfg = config::hollywood(&env.vars);
    Ok(hrender::hollywood_block(&hstate, &cfg))
}

/// `card show <id>` — one card's full read (D19): the header line,
/// then a `studio-new` card's draft — the cast's roles, candidates
/// and limits rendered, and the brief's lines. The TUI's "Show me"
/// view is this output.
pub fn card_show(common: &Common, env: &Env, id: u64) -> Result<String, Fail> {
    let base = base_of(common, env)?;
    let hdir = store::hollywood_dir(&base);
    let hstate = store::load_hstate(&hdir).map_err(store_fail)?;
    let card = hstate
        .cards
        .get(&id)
        .ok_or_else(|| Fail::refused(format!("no card 'c{id}'")))?;
    let mut out = format!(
        "c{} {} · {} — {}",
        card.seq,
        card.state.as_str(),
        card.action.label(),
        card.why
    );
    if !card.detail.is_empty() {
        out.push_str(&format!(" ({})", card.detail));
    }
    out.push_str(&format!("\nproposed by {}", card.by));
    if let backlot_model::hollywood::CardAction::StudioNew {
        studio,
        title,
        repo,
        cast,
        brief,
        ..
    } = &card.action
    {
        let cast_path = crate::hcard_new::resolve_draft(&hdir, cast);
        let brief_path = crate::hcard_new::resolve_draft(&hdir, brief);
        out.push_str(&format!("\nstudio: {studio}\ntitle: {title}\nrepo: {repo}"));
        match commands::read_cast(&cast_path) {
            Ok(cast) => {
                let role = |name: &str, a: &backlot_model::event::ActorIdentity| {
                    format!(
                        "{name}: {}/{}{}",
                        a.runner,
                        a.model,
                        a.effort
                            .as_deref()
                            .map(|e| format!("/{e}"))
                            .unwrap_or_default()
                    )
                };
                out.push_str("\ncast:");
                for (name, a) in [
                    ("producer", &cast.producer),
                    ("director", &cast.director),
                    ("writer", &cast.writer),
                ] {
                    out.push_str(&format!("\n  {}", role(name, a)));
                }
                if !cast.candidates.is_empty() {
                    let names: Vec<String> = cast
                        .candidates
                        .iter()
                        .map(|c| {
                            format!(
                                "{} ({}/{}{})",
                                c.name,
                                c.runner,
                                c.model,
                                c.effort
                                    .as_deref()
                                    .map(|e| format!("/{e}"))
                                    .unwrap_or_default()
                            )
                        })
                        .collect();
                    out.push_str(&format!("\ncandidates: {}", names.join(", ")));
                }
                let mut limits = Vec::new();
                if let Some(v) = cast.rework_cap {
                    limits.push(format!("rework_cap {v}"));
                }
                if let Some(v) = cast.stall_limit_s {
                    limits.push(format!("stall_limit_s {v}"));
                }
                if let Some(v) = cast.task_budget_s {
                    limits.push(format!("task_budget_s {v}"));
                }
                for (runner, cap) in &cast.limits {
                    limits.push(format!("{runner} ≤{cap} live"));
                }
                if !limits.is_empty() {
                    out.push_str(&format!("\nlimits: {}", limits.join(" · ")));
                }
            }
            Err(e) => out.push_str(&format!("\ncast {}: {}", cast_path.display(), e.message())),
        }
        match std::fs::read_to_string(&brief_path) {
            Ok(text) => {
                out.push_str(&format!("\nbrief {}:", brief_path.display()));
                let mut lines = text.lines();
                for _ in 0..50 {
                    match lines.next() {
                        Some(line) => out.push_str(&format!("\n  {line}")),
                        None => break,
                    }
                }
                if lines.next().is_some() {
                    out.push_str("\n  …");
                }
            }
            Err(e) => out.push_str(&format!("\nbrief {}: {e}", brief_path.display())),
        }
    }
    // A recast card shows both identities: the one it replaces (the
    // card's base) and the one it puts on.
    if let (
        backlot_model::hollywood::CardAction::Recast { person, actor, .. },
        backlot_model::hollywood::CardBase::RoleIdentity { actor: old },
    ) = (&card.action, &card.base)
    {
        out.push_str(&format!(
            "\n{person}: {} ({}) → {} ({})",
            backlot_model::recast::identity_text(old),
            old.permission_profile,
            backlot_model::recast::identity_text(actor),
            actor.permission_profile,
        ));
    }
    Ok(out)
}

/// `status --all`: every studio, one block each, then Hollywood's
/// cards and headline — the shape the expanded layer will draw (D16).
pub fn status_all(env: &Env, base: &Path, json: bool) -> Result<String, Fail> {
    let names = store::studios(base).map_err(|e| Fail::refused(e.to_string()))?;
    let mut blocks = Vec::new();
    let mut states = BTreeMap::new();
    for name in &names {
        let dir = store::studio_dir(base, name);
        let state = store::load_state(&dir).map_err(store_fail)?;
        if json {
            states.insert(name.clone(), state);
        } else {
            let now_ms = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_millis() as u64)
                .unwrap_or(0);
            blocks.push(render::status(
                name,
                &state,
                &crate::signals::failed_roles(&dir, &state),
                now_ms,
            ));
        }
    }
    let hstate = store::load_hstate(&store::hollywood_dir(base)).map_err(store_fail)?;
    let cfg = config::hollywood(&env.vars);
    if json {
        let j = serde_json::json!({
            "studios": states,
            "hollywood": hrender::hollywood_json(&hstate, &cfg),
        });
        serde_json::to_string_pretty(&j).map_err(|e| Fail::refused(e.to_string()))
    } else {
        blocks.push(hrender::hollywood_block(&hstate, &cfg));
        Ok(blocks.join("\n"))
    }
}
