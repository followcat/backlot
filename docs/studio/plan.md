# Studio plan

Each milestone ends with green CI and a short entry in `docs/studio/progress.md`: done, measured, not verified.

Acceptance is visible behaviour. A fix loop is capped at about two rounds. After that, accept with a documented gap, or
rescope. Prefer porting working code over new infrastructure.

| | Milestone | Acceptance |
|---|---|---|
| S0 | Identity: binary `backlot`, README stating the origin and the `hc` requirement, `progress.md` | `backlot --version`; TUI behaves as before; parity harness result unchanged |
| S1 | Ledger: `backlot-model` (events, reducer), state directory, recording commands | Replay reproduces state. Concurrent appends keep a total order. A correction changes the view, not the past. `backlot status` shows a scripted studio |
| S2 | Stage: `backlot stage` on `hc-proto`, role terminals with labels and env, summary document, single stage per core | In a scratch session with a fake runner: roles start, appear in `hc ls` and the left sidebar, and the document updates on change and at most every 2 s |
| S3 | Runner adapters: Claude Code, Codex, Devin, OpenCode. Launch, per-session flags, session ID capture, resume | `runners.md` table rows measured and updated. Unit tests against a fake runner. A real launch of each with a throwaway prompt, run by the supervisor |
| S4 | Panel (R17-R20, D11, D12): right column, closed, collapsed to a 16-column list of working roles, and expanded with collapsible role groups; work-state colours, marks and animation; scrollbar and scrolling; the people in each role; closed people kept and reopenable with `backlot reopen`; priority order (`backlot cast --priority`, changeable in the panel); focus by click and keys; full-screen at narrow widths; "stage not running"; backlot's own config file | Visible check at 200, 120, 80 and 40 columns: collapsed and expanded, a role group collapsed, each work state, the scrollbar, a change of priority, and a closed person reopened into their old session. With the panel closed, parity harness result unchanged |
| S5 | Closed loop: role prompts; plan, dispatch, cast, deliver, review, rework, report; light path; requirement revision midway | A real small requirement on a sample repository (`acme-shop`), end to end, visible in the panel. The ledger shows deliveries, rework and the requirement version each was made against |
| S6 | Resilience: stall detection, attention reasons, rework cap, restart recovery | In a scratch session: a stuck fake runner raises attention. The cap sends a task to the producer. After the stage and the hc server restart, the studio recovers from the ledger |
| S7 | First version: install, user docs, known limits | Install on a clean machine. Docs match behaviour. Release only on the user's approval |

Hollywood — one overseer above all studios (R21–R27, D13–D21), one milestone at a time, fake
runner only:

|| Milestone | Acceptance |
|---|---|---|
|| H1 | Model + CLI: Hollywood's own ledger, the `card.*`/`hollywood.reported`/`message.sent` events and reducer, the role rules, `hollywood propose` (incl. `studio-new` with a draft), `hollywood report`, `studio new --dry-run`, `card apply|cancel`, `tell hollywood`, `status --all`, `docs/studio/hollywood.md` | Unit tests for each rule accepted and refused; replay equals apply; `--dry-run` refuses each invalid draft and records nothing; `status --all` on a scripted pair of studios |
|| H2 | Two live studios on one core (never run before, see limits.md): the stage drives both; names, terminals, limits and notices stay apart | Scratch hc session, fake runner, two studios run to the end side by side; any bug found is fixed here, before Hollywood depends on it |
|| H3 | Stage: start/resume `bl-hollywood` from `[hollywood]`, its folder, role prompt and pre-allowed commands, notices to it, card execution and outcomes, the summary-doc section | Scratch hc session, fake runner, two studios: Hollywood starts, gets a notice from each, a proposed card applied by the test runs and records `card.done`; a stale card expires |
|| H4 | TUI layer: hidden / collapsed strip / expanded panel as floats, setup when unset, input line, Apply / Cancel / Show me, focus its terminal, select a studio to focus its producer, full-screen at narrow widths | Real TUI at 200/120/80/40 columns on a scratch core with fake runners; toggling resizes no pane; parity unchanged with the layer hidden and no `[hollywood]` |
|| H5 | One real run (only with the user's approval): Hollywood on a real runner over two small studios | Reports and one applied card, as visible behaviour |

Later, after real use:

| | Milestone |
|---|---|
| L1 | Several studios running at once (R13) |
| L2 | Experimental production (R14) |
| L3 | Casting that draws on actor records |

## Rules

Everything in `AGENTS.md` applies. In addition:

- **Never touch real sessions or agents.** Tests use a scratch hc session with a unique `HC_SESSION`, temporary XDG
  directories and a fake runner. Stop and delete scratch sessions afterwards.
- **Never change an account default.** Choose models through per-session flags only.
- **Core is off limits.** A milestone that seems to need a core change stops and reports instead.
- **Say what was not verified.** Coverage by unit tests alone is stated as such.
- **Public repository.** Neutral data only in code, fixtures and docs.
