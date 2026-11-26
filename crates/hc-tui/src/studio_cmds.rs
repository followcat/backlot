//! The `backlot` argv and Action for every studio command the TUI
//! runs (S3 §4): each carries `--studio <s> --as user` (`stage`,
//! `card` and `tell hollywood` stay global). The `NsRun` runner adds
//! this core's endpoint flags on local runs only — a remote call's
//! terminal reaches its own core through `HC_*`. The Action's
//! `done`/`name` wording is the toast text §3 sets.

use crate::action::{Action, mark};
use crate::studio::StudioKey;

/// `backlot <command> --studio <id> --as user` on `machine` — the
/// shared tail every studio call carries (§4).
pub(crate) fn studio_argv(command: &[String], studio: &StudioKey) -> Vec<String> {
    let mut argv = command.to_vec();
    argv.extend([
        "--studio".into(),
        studio.id.clone(),
        "--as".into(),
        "user".into(),
    ]);
    argv
}

/// A global `backlot` call (no `--studio`): `stage`, `card`,
/// `tell hollywood`, `records`. `--as user` rides the tail like the
/// studio calls — the ledger writes the user as the actor.
pub(crate) fn plain_argv(command: &[String]) -> Vec<String> {
    let mut argv = command.to_vec();
    argv.extend(["--as".into(), "user".into()]);
    argv
}

fn action(label: String, done: String, name: String, key: &StudioKey, mk: String) -> Action {
    Action {
        label,
        done,
        name,
        mark: mk,
        argv: Vec::new(),
        machine: key.machine.clone(),
        stdin: None,
        view: false,
    }
}

/// `task pause|resume|abandon|takeover --task <id> --reason <r>`
/// (§4) — `reason` stays optional for `takeover`, matching the
/// parser.
pub(crate) fn task(key: &StudioKey, task: &str, verb: &str, reason: Option<&str>) -> Action {
    let mut command = vec!["task".into(), verb.into(), "--task".into(), task.into()];
    if let Some(reason) = reason.filter(|r| !r.is_empty()) {
        command.extend(["--reason".into(), (*reason).into()]);
    }
    let mut a = action(
        format!("{verb}ing {task}"),
        format!("{verb}d task {task}"),
        format!("{verb} task {task}"),
        key,
        mark::task(key, task),
    );
    a.argv = studio_argv(&command, key);
    a
}

/// `close --person <role>` (§4).
pub(crate) fn close(key: &StudioKey, role: &str) -> Action {
    let mut a = action(
        format!("closing {role}"),
        format!("closed {role}"),
        format!("close {role}"),
        key,
        mark::role(key, role),
    );
    a.argv = studio_argv(&["close".into(), "--person".into(), role.into()], key);
    a
}

/// `restart --person <role> [--fresh]` (L2) — the stage reloads the
/// seat in place, resuming its session unless `fresh`.
pub(crate) fn restart(key: &StudioKey, role: &str, fresh: bool) -> Action {
    let word = if fresh {
        "restarting fresh"
    } else {
        "restarting"
    };
    let mut a = action(
        format!("{word} {role}"),
        format!("restarted {role}"),
        format!("restart {role}"),
        key,
        mark::role(key, role),
    );
    let mut command = vec!["restart".into(), "--person".into(), role.into()];
    if fresh {
        command.push("--fresh".into());
    }
    a.argv = studio_argv(&command, key);
    a
}

/// `restart --person hollywood [--fresh]` — the overseer's own ledger,
/// so no `--studio` rides the tail.
pub(crate) fn restart_hollywood(fresh: bool) -> Action {
    let mut argv = vec!["restart".into(), "--person".into(), "hollywood".into()];
    if fresh {
        argv.push("--fresh".into());
    }
    argv.extend(["--as".into(), "user".into()]);
    Action {
        label: if fresh {
            "restarting hollywood fresh".into()
        } else {
            "restarting hollywood".into()
        },
        done: "restarted hollywood".into(),
        name: "restart hollywood".into(),
        mark: mark::hollywood(),
        argv,
        machine: None,
        stdin: None,
        view: false,
    }
}

/// `recast --person <role> --runner <r> --model <m> [--effort <e>]`
/// (L3) — the new identity takes over on the stage's next tick.
pub(crate) fn recast(
    key: &StudioKey,
    role: &str,
    runner: &str,
    model: &str,
    effort: Option<&str>,
) -> Action {
    let mut command = vec![
        "recast".into(),
        "--person".into(),
        role.into(),
        "--runner".into(),
        runner.into(),
        "--model".into(),
        model.into(),
    ];
    if let Some(effort) = effort {
        command.extend(["--effort".into(), effort.into()]);
    }
    let mut a = action(
        format!("recasting {role}"),
        format!("recast {role} as {runner}"),
        format!("recast {role}"),
        key,
        mark::role(key, role),
    );
    a.argv = studio_argv(&command, key);
    a
}

/// `studio prune <studio>` (L7) — after its `--dry-run` plan. On a
/// server the dialog's reviewed `token` rides along.
pub(crate) fn prune(key: &StudioKey, token: Option<&str>) -> Action {
    let mut a = action(
        format!("pruning {}", key.id),
        format!("pruned work areas of {}", key.id),
        format!("prune {}", key.id),
        key,
        mark::studio(key),
    );
    a.argv = ["studio", "prune", &key.id, "--as", "user"]
        .into_iter()
        .map(str::to_owned)
        .collect();
    push_token(&mut a.argv, token);
    a
}

/// A reviewed plan's token — a server-only hidden flag the queue path
/// strips and a local run never carries.
fn push_token(argv: &mut Vec<String>, token: Option<&str>) {
    if let Some(token) = token {
        argv.push("--confirm-token".into());
        argv.push(token.to_owned());
    }
}

/// `reopen --person <role>` (§4).
pub(crate) fn reopen(key: &StudioKey, role: &str) -> Action {
    let mut a = action(
        format!("reopening {role}"),
        format!("reopened {role}"),
        format!("reopen {role}"),
        key,
        mark::role(key, role),
    );
    a.argv = studio_argv(&["reopen".into(), "--person".into(), role.into()], key);
    a
}

/// `approve --person <role> --decision <d>` — also the D9 override
/// (`--as user` rides the tail).
pub(crate) fn decide(key: &StudioKey, role: &str, allow: bool) -> Action {
    let word = if allow { "allow" } else { "deny" };
    let mut a = action(
        format!("{word}ing {role}"),
        format!("{word}ed {role} ({})", key.id),
        format!("{word} {role}"),
        key,
        mark::role(key, role),
    );
    a.argv = studio_argv(
        &[
            "approve".into(),
            "--person".into(),
            role.into(),
            "--decision".into(),
            word.into(),
        ],
        key,
    );
    a
}

/// `attention clear <seq>` (§4).
pub(crate) fn clear_attention(key: &StudioKey, role: &str, seq: u64) -> Action {
    let mut a = action(
        format!("clearing #{seq}"),
        format!("cleared attention #{seq}"),
        format!("clear attention #{seq}"),
        key,
        mark::role(key, role),
    );
    a.argv = studio_argv(&["attention".into(), "clear".into(), seq.to_string()], key);
    a
}

pub(crate) fn finish(key: &StudioKey, reason: Option<&str>) -> Action {
    let mut command = vec!["studio".into(), "finish".into()];
    if let Some(reason) = reason.filter(|r| !r.is_empty()) {
        command.extend(["--reason".into(), (*reason).into()]);
    }
    let mut a = action(
        format!("finishing {}", key.id),
        format!("finished {}", key.id),
        format!("finish {}", key.id),
        key,
        mark::studio(key),
    );
    a.argv = studio_argv(&command, key);
    a
}
fn stage(verb: &str, label: String, done: String, name: String, machine: Option<String>) -> Action {
    Action {
        label,
        done,
        name,
        mark: mark::stage(machine.as_deref()),
        argv: vec!["stage".into(), verb.into()],
        machine,
        stdin: None,
        view: false,
    }
}
pub(crate) fn stage_start(machine: Option<String>) -> Action {
    let m = machine.as_deref().unwrap_or("Local");
    stage(
        "start",
        format!("starting stage {m}"),
        format!("stage started on {m}"),
        "start stage".into(),
        machine,
    )
}
pub(crate) fn stage_stop(machine: Option<String>) -> Action {
    let m = machine.as_deref().unwrap_or("Local");
    stage(
        "stop",
        format!("stopping stage {m}"),
        format!("stage stopped on {m}"),
        "stop stage".into(),
        machine,
    )
}

/// `tell <role> --text <msg>` (§4, G2).
pub(crate) fn tell(key: &StudioKey, role: &str, text: &str) -> Action {
    let mut a = action(
        format!("telling {role}"),
        format!("queued note to {role}"),
        format!("tell {role}"),
        key,
        mark::role(key, role),
    );
    a.argv = studio_argv(
        &["tell".into(), role.into(), "--text".into(), text.into()],
        key,
    );
    a
}

/// `req revise --stdin --summary <s>` (§4, G2) — `body` pipes
/// through the runner's stdin, local and remote alike.
pub(crate) fn revise(key: &StudioKey, summary: &str, body: String) -> Action {
    let mut a = action(
        "revising requirement".to_owned(),
        format!("revised requirement ({})", key.id),
        "revise requirement".to_owned(),
        key,
        mark::studio(key),
    );
    a.argv = studio_argv(
        &[
            "req".into(),
            "revise".into(),
            "--stdin".into(),
            "--summary".into(),
            summary.into(),
        ],
        key,
    );
    a.stdin = Some(body);
    a
}

/// `studio cast show` — the effective cast as a cast file, read into the
/// TUI for the `Edit cast…` editor. A view action (`view: true`), so its
/// stdout lands in the dialog rather than a toast.
pub(crate) fn cast_show(key: &StudioKey) -> Action {
    let mut a = action(
        "reading the cast".to_owned(),
        format!("read the cast of {}", key.id),
        "read cast".to_owned(),
        key,
        mark::cast_show(key),
    );
    a.argv = studio_argv(&["studio".into(), "cast".into(), "show".into()], key);
    a.view = true;
    a
}

/// `studio cast set --file - [--dry-run]` with the file's content on
/// stdin — the same bytes on a local core and over the server queue,
/// where content travels and a client path never does.
pub(crate) fn cast_set(key: &StudioKey, body: String, dry_run: bool) -> Action {
    let (label, done, name) = if dry_run {
        (
            "checking the cast",
            format!("checked the cast of {}", key.id),
            "check cast",
        )
    } else {
        (
            "setting the cast",
            format!("set the cast of {}", key.id),
            "set cast",
        )
    };
    let mut a = action(
        label.to_owned(),
        done,
        name.to_owned(),
        key,
        mark::studio(key),
    );
    let mut command = vec!["studio".to_owned(), "cast".to_owned(), "set".to_owned()];
    command.extend(["--file".into(), "-".into()]);
    if dry_run {
        command.push("--dry-run".into());
    }
    a.argv = studio_argv(&command, key);
    a.stdin = Some(body);
    a
}

/// `card apply <id>` on `machine` (`None` is local).
pub(crate) fn card_apply_on(machine: Option<String>, id: &str) -> Action {
    Action {
        label: format!("applying {id}"),
        done: format!("applied card {id}"),
        name: format!("apply card {id}"),
        mark: mark::card(id),
        argv: plain_argv(&["card".into(), "apply".into(), id.into()]),
        machine,
        stdin: None,
        view: false,
    }
}

/// `card cancel <id> [--reason <r>]` on `machine` (`None` is local).
pub(crate) fn card_cancel_on(machine: Option<String>, id: &str, reason: Option<&str>) -> Action {
    let mut command = vec!["card".into(), "cancel".into(), id.into()];
    command.extend([
        "--reason".into(),
        reason.unwrap_or("cancelled from the call sheet").into(),
    ]);
    Action {
        label: format!("cancelling {id}"),
        done: format!("cancelled card {id}"),
        name: format!("cancel card {id}"),
        mark: mark::card(id),
        argv: plain_argv(&command),
        machine,
        stdin: None,
        view: false,
    }
}

/// `card show <id>` on `machine` (`None` is local); `view` opens the read instead of a toast.
pub(crate) fn card_show_on(machine: Option<String>, id: &str) -> Action {
    Action {
        label: format!("card show {id}"),
        done: format!("showed card {id}"),
        name: format!("card show {id}"),
        mark: mark::card(id),
        argv: plain_argv(&["card".into(), "show".into(), id.into()]),
        machine,
        stdin: None,
        view: true,
    }
}

/// `priority --task <id> --level <l>` (S2 §4 through the tracker).
pub(crate) fn priority(key: &StudioKey, task: &str, level: &str) -> Action {
    let mut a = action(
        format!("priority {task} {level}"),
        format!("priority {task} → {level}"),
        format!("priority {task}"),
        key,
        mark::task(key, task),
    );
    a.argv = studio_argv(
        &[
            "priority".into(),
            "--task".into(),
            task.into(),
            "--level".into(),
            level.into(),
        ],
        key,
    );
    a
}

/// `hollywood setup --runner <r> --model <m> [--effort <e>]`
/// ` [--key <k>] [--default-cast <d>]` — this core's own call; the
/// in-TUI dialog's apply builds the cfg, `key`/`default_cast` ride
/// along verbatim so a reconfigure loses nothing.
pub(crate) fn hw_setup(cfg: &backlot_stage::config::HollywoodCfg) -> Action {
    let mut command = vec![
        "hollywood".into(),
        "setup".into(),
        "--runner".into(),
        cfg.runner.clone(),
        "--model".into(),
        cfg.model.clone(),
    ];
    if let Some(e) = &cfg.effort {
        command.extend(["--effort".into(), e.clone()]);
    }
    if let Some(k) = &cfg.key {
        command.extend(["--key".into(), k.clone()]);
    }
    if let Some(d) = &cfg.default_cast {
        command.extend(["--default-cast".into(), d.clone()]);
    }
    Action {
        label: "configuring hollywood".into(),
        done: "configured hollywood".into(),
        name: "hollywood setup".into(),
        mark: mark::hollywood_setup(),
        argv: plain_argv(&command),
        machine: None,
        stdin: None,
        view: false,
    }
}

/// `approve --person hollywood --decision allow|deny` — the answer to
/// a prompt on Hollywood's own screen; no `--studio`, its own ledger.
pub(crate) fn hollywood_approve(allow: bool) -> Action {
    let word = if allow { "allow" } else { "deny" };
    Action {
        label: format!("{word}ing hollywood"),
        done: format!("{word}ed hollywood"),
        name: format!("{word} hollywood"),
        mark: mark::hollywood(),
        argv: plain_argv(&[
            "approve".into(),
            "--person".into(),
            "hollywood".into(),
            "--decision".into(),
            word.into(),
        ]),
        machine: None,
        stdin: None,
        view: false,
    }
}

/// `tell hollywood "<text>"` — the headline's own line (D20).
pub(crate) fn tell_hollywood(text: &str) -> Action {
    Action {
        label: "telling hollywood".into(),
        done: "told hollywood".into(),
        name: "tell hollywood".into(),
        mark: mark::hollywood(),
        argv: plain_argv(&["tell".into(), "hollywood".into(), text.into()]),
        machine: None,
        stdin: None,
        view: false,
    }
}

/// `hollywood show <seq>` — full report body or message text, opened
/// in the read-only `CardView`.
pub(crate) fn hollywood_show(seq: u64) -> Action {
    hollywood_exchange(seq, false)
}

/// One Hollywood exchange entry in the read-only `CardView`: a report
/// or message reads `hollywood show <seq>`, a card line `card show
/// c<seq>`. Both keep the same `hollywood #{seq}` name and mark so the
/// Reader's sibling stepping walks the whole conversation.
pub(crate) fn hollywood_exchange(seq: u64, card: bool) -> Action {
    Action {
        label: format!("reading #{seq}"),
        done: format!("read #{seq}"),
        name: format!("hollywood #{seq}"),
        mark: mark::hollywood_entry(seq),
        argv: if card {
            plain_argv(&["card".into(), "show".into(), format!("c{seq}")])
        } else {
            plain_argv(&["hollywood".into(), "show".into(), seq.to_string()])
        },
        machine: None,
        stdin: None,
        view: true,
    }
}

/// `records --studio <s>` — the actor-records read, opened as a
/// view on success.
pub(crate) fn records(key: &StudioKey) -> Action {
    let mut a = action(
        format!("reading records {}", key.id),
        format!("read records {}", key.id),
        format!("records {}", key.id),
        key,
        mark::studio(key),
    );
    a.argv = studio_argv(&["records".into()], key);
    a.view = true;
    a
}

/// `report show --file <rel> --studio <s> --as user` — streams a
/// studio message or report body into the read-only `CardView`.
pub(crate) fn report_show(key: &StudioKey, seq: u64, file: &str) -> Action {
    let mut a = action(
        format!("reading #{seq}"),
        format!("read #{seq}"),
        format!("{} #{seq}", key.id),
        key,
        mark::studio_talk(key, seq),
    );
    a.argv = studio_argv(
        &["report".into(), "show".into(), "--file".into(), file.into()],
        key,
    );
    a.view = true;
    a
}

/// `studio finish --force` (L5) — the empty-studio guard's answer.
pub(crate) fn finish_force(key: &StudioKey) -> Action {
    let mut a = action(
        format!("finishing {}", key.id),
        format!("finished {}", key.id),
        format!("finish {}", key.id),
        key,
        mark::studio(key),
    );
    a.argv = studio_argv(&["studio".into(), "finish".into(), "--force".into()], key);
    a
}

/// `studio restore <archived> --name <name> [--dry-run]` (M3) — the
/// archived ledger comes back as a finished studio.
pub(crate) fn restore(
    machine: Option<String>,
    archived: &str,
    name: &str,
    dry_run: bool,
    token: Option<&str>,
) -> Action {
    let mut argv = vec![
        "studio".into(),
        "restore".into(),
        archived.to_owned(),
        "--name".into(),
        name.to_owned(),
    ];
    if dry_run {
        argv.push("--dry-run".into());
    }
    argv.extend(["--as".into(), "user".into()]);
    push_token(&mut argv, token);
    Action {
        label: format!("restoring {archived}"),
        done: format!("restored {archived}"),
        name: format!("restore {archived}"),
        mark: format!("studio:{archived}"),
        argv,
        machine,
        stdin: None,
        view: false,
    }
}

pub(crate) fn delete_studio(key: &StudioKey, force: bool, token: Option<&str>) -> Action {
    let mut a = action(
        format!("deleting {}", key.id),
        format!("deleted studio {}", key.id),
        format!("delete studio {}", key.id),
        key,
        mark::studio(key),
    );
    a.argv = ["studio", "delete", &key.id, "--json", "--as", "user"]
        .into_iter()
        .chain(force.then_some("--force"))
        .map(str::to_owned)
        .collect();
    push_token(&mut a.argv, token);
    a
}
