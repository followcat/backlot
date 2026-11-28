---
name: backlot
description: Drive backlot studios — the coding agents a person directs like a film cast. Use when the person asks about their studios, roles, Hollywood, the stage, tasks, deliveries or reviews, or wants a studio started, told, asked, checked or finished.
---

# backlot

backlot is a terminal client for the `hc` multiplexer with a studio layer. A studio is one
requirement worked by a cast of agent CLIs: a producer you talk to, a director who splits,
reviews and integrates the work, a writer who plans, and actors who do the tasks. An
append-only ledger records every step, and talking never moves a task — a `backlot` command does.

## Find the studio

```sh
backlot studio list
backlot --session work studio list
backlot --server host-a studio list
backlot --remote host-a studio list
```

`--session <s>` reads another core's state on this machine. `--server <host>[/<session>]`
reaches a backlot server on another machine, `--remote <host>` reads that machine's studios
over `ssh`. Most commands take the same flags, plus `--studio <name>` (or `BACKLOT_STUDIO`).

## Identify yourself

Before your first write, tell the ledger which agent you are:

```sh
BACKLOT_VIA=claude-code backlot tell producer --text "Please add search." --studio acme-shop --as user
```

`BACKLOT_VIA=<agent>` or `--via <agent>` records `agent:<agent>` on events you write
`--as user`; the records then read `user (via claude-code)`. Use the agent that is actually
running you: `claude-code`, `codex`, `cursor`, `devin`, `command-code`, `opencode`, or a
similar lowercase name matching `[a-z0-9-]{1,32}`.

**Always act `--as user`.** Never pass `--as <role>` or an actor key, and never run a role's
verbs: a role's work belongs in that role's own terminal, and running it from outside forges
that role. To get a role's work done, `tell` that role.

## Read before you act

```sh
backlot status --studio acme-shop --json
backlot records --studio acme-shop
backlot msg show 3 --studio acme-shop
backlot guide
backlot guide agents
```

`status` shows the requirement, every task and the `next:` steps still owed; `records` shows
what each actor delivered and how reviews went; `msg show <seq>` prints one recorded message,
ask, design note or report. `backlot guide <topic>` prints the installed version's own docs.

## Act

Only what the person does — every one `--as user`:

```sh
backlot studio new --studio acme-shop --title "Shop" --repo /home/user/acme-shop --cast /home/user/cast.toml --ask "Build the checkout flow" --dry-run --as user
backlot stage start --as user
backlot tell producer --text "Please add search." --studio acme-shop --as user
backlot tell director --file /home/user/note.md --studio acme-shop --as user
backlot req revise --file /home/user/req-v2.md --summary "second version" --studio acme-shop --as user
backlot priority --task t1 --level high --studio acme-shop --as user
backlot task pause --task t1 --reason "waiting on design" --studio acme-shop --as user
backlot task resume --task t1 --reason "design settled" --studio acme-shop --as user
backlot task takeover --task t1 --reason "the actor is stuck" --studio acme-shop --as user
backlot task abandon --task t1 --reason "no longer needed" --studio acme-shop --as user
```

Run `studio new` without `--dry-run` only when the person asked for that studio. `--ask`
queues the person's first message; `--dry-run` validates and records nothing. A role's verbs
— `task new`, `cast`, `plan publish`, `dispatch`, `deliver`, `review`, `ask` — are that
role's own work: `tell` the role and let it run them in its terminal.

## Person-only

These refuse every non-person caller. Run one only when the person asked for **that action**
in this conversation, and say the exact command before running it. Otherwise prepare the
file, a dry-run command or the card request text, and tell the person.

<!-- person-only:start -->
- Apply or cancel cards (`backlot card apply c<n>`, `backlot card cancel c<n>`).
- Change the studio cast, recast any role or actor, or restart Hollywood (`backlot studio cast set --file <file>` without `--dry-run`, `backlot recast --person <role> --runner <r> --model <m> --reason <r>`, `backlot restart --person hollywood --reason <r>`).
- Add or remove studio checks (`backlot studio check add --name <n> --run <r>`, `backlot studio check remove --name <n>`).
- Sign off or skip opening design (`backlot design sign`, including `--skip` and `--no-checks`).
- Finish, reopen, delete, or restore a studio (`backlot studio finish --reason <r>`, `backlot studio reopen --reason <r>`, `backlot studio delete`, `backlot studio restore <archived-name>`).
- Configure, switch, or cancel Hollywood setup (`backlot hollywood setup --runner <r> --model <m>`, `backlot hollywood switch --status` or `--cancel`).
- Answer non-actor, self, or user-owned permission prompts (`backlot approve --person <role> --decision allow`, or `deny`, for `producer`, `director`, `writer`, `hollywood`, self, or `needs_user` prompts).
- Accept a delivery while declared `[[checks]]` still have no recorded run (`backlot review --task <task> --delivery 1 --verdict accepted --conclusion <r>`).
- Revoke permissions (`backlot perms revoke <seq>`).
- Start, stop, adopt, or manage the server (`backlot server start`, `run`, `stop`, `adopt`, `service`).
- Show or remove approved repository notes directly (`backlot repo-note show`, `backlot repo-note remove --id <n>`; propose notes with `backlot repo-note propose --id <n> --text <r> --why <r>`).
- Acknowledge a user ask (`backlot ask acknowledged --person <role> --target <seq>`).
- Protected-branch, default-branch, tag, unrecorded/new-remote, `--all`/`--branches`/glob/matching/detached git pushes and `gh pr create` (prepare a user card with `backlot ask --run --why <r> -- git push`), plus hard-refused outward commands (force/delete/mirror pushes, mutating `gh`, `hc` control, sockets/daemons, remote `ssh`/`scp`/`rsync`, `npm publish`/`cargo publish`).
<!-- person-only:end -->

## Never

- Act as a role: always `--as user`, never `--as <role>` or an actor key, and never run a role's
  verbs (`task new`, `cast`, `plan publish`, `dispatch`, `deliver`, `review`, `ask`). Tell the
  role instead.
- Answer a role's permission prompt or apply, cancel or withdraw a card unless the person asked
  for that in this conversation.
- Kill a process, restart the hc server, or stop the stage to get unstuck.
- Edit the ledger or any state file under the state directory by hand.
- Change an agent's model with `/model` or by editing the cast file directly.
- Guess a requirement, a repository path or a model id the person did not give you.

## More

`backlot guide <topic>` prints the docs of the installed version. Topics: `agents`, `design`,
`guide`, `hollywood`, `ledger`, `limits`, `metrics`, `panel`, `plan`, `progress`, `requests`,
`requirements`, `runners`, `server`, `testing`. `backlot guide --grep <word>` searches them all.
