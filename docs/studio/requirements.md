# Studio requirements

backlot is a terminal client for `hc` with a studio layer for directing coding agents. hc-tui stays a plain terminal
client; everything about multi-agent coordination lives in this project only.

Numbers R01-R16 follow the original requirement list. R17-R20 were added by the user on 2026-11-09 and specify the
panel's interaction. Model names that appeared in the original examples are examples, not product constraints.

## Requirements

| # | Requirement | First version |
|---|---|---|
| R01 | Built on `hc`, with the same functions as the herdr TUI. Inherited from hc-tui through `upstream`. | Inherited |
| R02 | Extended far enough to carry its own name. | Done: `backlot` |
| R03 | Organized like a film studio. Other structures were to be compared. | See `design.md` |
| R04 | A studio panel on the right side, with producer, director, actors and writer. | Yes |
| R05 | Before development starts, set agent, model and adjustable thinking depth for each role. | Yes |
| R06 | The user normally talks to the producer, who understands the requirement and plans development and execution. Other roles stay reachable. | Yes |
| R07 | The producer discusses generalization, extensibility and alternatives with the writer. | Yes |
| R08 | The producer asks the director to schedule actors. The director checks the work against the writer's content. | Yes |
| R09 | The director reports to the producer at nodes. The producer reports to the user. | Yes |
| R10 | While actors work, the user can keep talking to the producer, adding requirements and correcting the plan. | Yes |
| R11 | The user chooses candidate actors at the start. The director selects several from those candidates to do the work. | Yes |
| R12 | The producer and director evaluate actors, as model plus thinking depth, from the finished work. | Yes |
| R13 | Several studios in the panel, scrollable, with click or switch to see what each person is doing. | One studio; panel is built to scroll |
| R14 | An "experimental production" (试验片) option that switches on a research-level process. | Later |
| R15 | Actors are full agents that complete sub-tasks on their own: reading the repo, editing, running, testing and fixing. | Yes |
| R16 | Evaluation comes from each delivery: the upstream reviewing it, correcting it, what changed, and whether it was reworked repeatedly. Every delivery counts. | Yes |
| R17 | The panel can collapse. Its title names the part that is working. Work states have clearly different colours and marks, ideally animated (spinning, slow blinking). | Yes |
| R18 | The panel has a scrollbar and scrolls. | Yes |
| R19 | Expanded, the panel shows the people in each role. A person whose work is finished is closed, which frees their resources, but is not deleted, so the work can be continued later. | Yes |
| R20 | Several people in one role are ordered by priority. | Yes |
| R21 | A floating Hollywood layer at the top of the screen: hidden, collapsed (a one-row strip) or expanded (a panel). It is drawn over the panes and never changes their size. | Layer is H4 |
| R22 | It works once its agent is set: runner, model and thinking depth, like a role (R05). Until then it offers the setup and nothing else. | Config in H1; agent start is H3 |
| R23 | Hollywood is told what needs the user in every studio (attention, user-owned prompts, a studio finished or failed, a delivery accepted, a stall it cannot route) and reports it, as one line in the strip and in its conversation. | Notices are H3 |
| R24 | On request, Hollywood reports on any or all studios: state, progress, what each producer and director said last, open attention, records. | `status`/`records` with `--studio`/`--all` in H1 |
| R25 | Hollywood manages through cards: it proposes an action, the user applies or cancels it in the panel; only an applied card changes anything. | Model and CLI in H1; panel is H4 |
| R26 | Hollywood talks to the user and, when the user asks, to studio producers. It never talks to directors, writers or actors, and never acts inside a studio itself. | Role rules in H1 |
| R27 | The user creates a studio by telling Hollywood in plain words what they want (the goal, the repository, who should work on it). Hollywood turns it into a complete studio proposal, asks back for anything it cannot settle, and the studio starts when the user applies its card. The panel has an input line for this, so the user need not switch to Hollywood's terminal. | `studio new --dry-run` and the `studio-new` card in H1; the input line is H4 |

## Confirmed decisions

| # | Decision |
|---|---|
| D1 | The project is named `backlot`, in the public repository `followcat/backlot`. |
| D2 | The studio changes zero lines of hc core. See the constraint below. |
| D3 | The first version is one studio running a real closed loop: talk to the producer, the director dispatches, actors deliver, review and rework are recorded, the panel shows each role's state, and requirements can change midway. Several studios and experimental production come afterwards. Neither is dropped. |
| D4 | A studio is one requirement from start to delivery, bound to one repository and one set of candidate actors. One repository can run several studios. Actor records accumulate across studios. |
| D5 | Producer and director are long-lived sessions. The writer starts only to discuss a plan. Actors start per task. Small tasks can take a light path: the producer dispatches to one actor, or one strong agent does the whole job alone. |
| D6 | With the studio panel closed, the screen stays cell-identical to herdr and the parity harness remains the regression gate. An open panel takes width on the right. At narrow and phone widths the panel does not sit beside the panes; it becomes a full-screen view. |
| D7 | "Several actors" (R11) means different actors take different tasks that can each be delivered on their own, and the director integrates them. Several actors racing on one task is left for later and may suit experimental production. |
| D8 | Evaluation in the first version is the factual record plus a written conclusion from the director on each delivery. There is no numeric score or grade yet. |
| D9 | The director may approve an actor's permission prompts inside that actor's own work area. Anything touching real sessions, production, accounts, global settings, or destructive outside the work area goes to the user — as does a prompt on a non-actor role or one whose screen reaches for the `BACKLOT_*` identity variables or environment-clearing flags (`env -i`, `--ignore-environment`). |
| D10 | Each actor has its own worktree and branch. The director integrates accepted deliveries into a studio branch and runs the project's checks there. Merging into the user's own branches stays with the user. |
| D11 | Priority (R20) affects only the display order in the panel, never scheduling. The director sets it per task when casting, the default is normal, and the user can change it in the panel. |
| D12 | The collapsed panel (R17) is a narrow column of about 16 columns listing the working roles and their status marks. Each role group can also collapse on its own. |
| D13 | One Hollywood per core, a long-lived role `hollywood` outside any studio, started and resumed by the stage like a producer (terminal `bl-hollywood`, labels `backlot.role=hollywood`, no `backlot.studio`). Its folder is `<state dir>/hollywood/` with `AGENTS.md` (its role), `memory.md` (kept across restarts and runner changes, never overwritten) and **its own ledger** in the studio ledger's format. It is not a reserved studio: studio lists, limits and studio rules never see it. |
| D14 | Its agent is set in `~/.config/backlot/config.toml` `[hollywood]` (`runner`, `model`, `effort`), or from the panel's setup, which writes that table. Any runner backlot supports. Changing the runner starts a new conversation; the memory stays. |
| D15 | **Floating, three states.** Hidden draws nothing (parity). Collapsed is a one-row strip floating on the top row; expanded is a panel that drops down from the top. Both are drawn over the screen like the existing floats (`app/float.rs`, `float_draw.rs`): the pane layout is never recomputed, so toggling causes no resize in any program. With no `[hollywood]` table the layer starts hidden; once set, it starts collapsed. |
| D15a | **Key `alt+y`**, a direct key with no prefix (user's choice, confirmed 2026-11-15 after weighing `alt+h`; no backlot or herdr default uses it). It switches collapsed and expanded, and shows the layer when hidden; `x` in the expanded panel hides it. `[hollywood] key` changes it (a direct binding like `alt+y`, or `prefix+<key>` for a chord). It is bound only while `[hollywood]` exists, so without it Alt+Y reaches the focused program and parity is unchanged. Known costs, in `docs/studio/hollywood.md`: (a) while bound, Alt+Y no longer reaches the program in the pane (readline and Emacs use M-y for yank-pop); (b) on a terminal without the kitty keyboard protocol, Esc followed by `y` within the escape window (150 ms while the mouse is captured, `raw_input.rs`) is read as Alt+Y; rare in practice, and a user who hits it sets `key = "prefix+h"`; (c) on macOS the terminal must send Option as Meta. The studio panel keeps `prefix+y`. The state is per client. Once the panel has focus, its own keys are plain letters and arrows; Esc collapses it. |
| D16 | Collapsed strip (1 row, sized to its content, at most a third of the width, at the right end of the top row so tab names stay readable): Hollywood's mark and state, studio counts by state, the count of open cards, then the newest headline, cut to fit. Expanded panel (up to half the height, scrollable): one row per studio (name, state, attention count, doing), Hollywood's last report lines, the open cards with Apply / Cancel / Show me, and the input line. At narrow and phone widths, expanded is a full-screen view, like the studio panel. The studio rows come from the stage's summary document, so they show even when Hollywood's agent is down. |
| D17 | Cards: `backlot hollywood propose <action> …` records `card.proposed` in Hollywood's ledger with the exact action and a one-line why; the panel shows it; the user's Apply records `card.applied` and the stage carries the action out and records the result (`card.done` or `card.failed` with the reason); Cancel records `card.cancelled`. Hollywood is told the outcome. A card expires when its studio or task moved past what it was made for (its base no longer holds), recorded `card.stale`. |
| D17a | **What holds the gate.** Identity in backlot is taken from the environment (`BACKLOT_ROLE`, `--as`), so the CLI cannot prove that `card apply` came from the user: a role terminal could run it. The gate is therefore (1) the role rules, which refuse `card apply` when `BACKLOT_ROLE` is set, and (2) the runner's own permission prompt: Hollywood's launch pre-allows only `backlot status`, `records`, `tell <studio> producer`, `hollywood propose`, `hollywood report` and `studio new --dry-run`, so any other command stops at a prompt the user sees. `docs/studio/hollywood.md` says plainly that this is a working rule, not a security boundary. |
| D18 | Card actions, first version: start a studio (`studio new` with repo, title, cast file), close or reopen a studio's finished people, change a task's priority, pause / resume / abandon a task. Never: answer a permission prompt for the user (left out of the first version: the user answers those in the studio), merge into the user's branches, change settings outside backlot, anything a `BACKLOT_*` rule keeps for the user alone. |
| D19 | Studio from natural language (R27): Hollywood writes a draft in its own folder, `drafts/<studio>/cast.toml` and `drafts/<studio>/brief.md` (the requirement in the user's words plus what was settled), and checks it with `backlot studio new --dry-run` (every `studio new` validation — name, repo is a git repository the user owns, runners installed, caps that can seat the fixed roles — with nothing recorded). Only a draft that passes becomes a card: `hollywood propose studio-new --studio <name> --title … --repo … --cast <draft> --brief <draft>`. The card shows name, title, repo, the runner and model of producer, director and writer, the candidate actors, the limits, and the brief's first lines; *Show me* opens the draft files. Apply copies the draft into the studio, records `studio.created`, and the brief is the producer's first message. Hollywood never guesses a repository path or a runner: what the user did not say comes from `[hollywood] default_cast` in the config, else the last studio's cast, else Hollywood asks. |
| D20 | The panel's input line (`i` or a click) sends one line with `backlot tell hollywood` as the user: recorded `message.sent`, delivered by the stage's guarded send (only when Hollywood's terminal is ready for text, never into a dialog). It does not write to the terminal directly. Multi-line or long talk stays in Hollywood's terminal. |
| D21 | Hollywood's words reach the layer through `backlot hollywood report "<headline>" [--body <file>]`, recorded `hollywood.reported` in its ledger. The strip shows the newest headline, the panel the last few. The TUI never scrapes Hollywood's terminal. |

## Hard constraint: no changes to hc core

- Use only hc's public API.
- Role sessions are ordinary hc terminals.
- Scheduling is this project's own process, itself run in an hc terminal.
- The ledger and other durable state live in this project's own state directory.
- The panel's live summary is a small core document, written with `doc.set` and read through the watcher.
- If a capability is missing, solve it in the client first. If that is impossible, propose only a general capability
  upstream. Studio concepts never go into core.

## Non-goals for the first version

- Several studios running at once (R13 beyond one studio) and experimental production (R14).
- Automatic casting from past records. The director uses the records; nothing selects for it.
- Any claim that a mixed team beats one strong agent. "One strong agent does it" is a first-class path, so the
  comparison can come from real tasks.
