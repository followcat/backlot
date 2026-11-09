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
| S4 | Panel: right column, toggle, scrolling, focus by click and keys, full-screen at narrow widths, "stage not running", backlot's own config file | Visible check at 200, 120, 80 and 40 columns. With the panel closed, parity harness result unchanged |
| S5 | Closed loop: role prompts; plan, dispatch, cast, deliver, review, rework, report; light path; requirement revision midway | A real small requirement on a sample repository (`acme-shop`), end to end, visible in the panel. The ledger shows deliveries, rework and the requirement version each was made against |
| S6 | Resilience: stall detection, attention reasons, rework cap, restart recovery | In a scratch session: a stuck fake runner raises attention. The cap sends a task to the producer. After the stage and the hc server restart, the studio recovers from the ledger |
| S7 | First version: install, user docs, known limits | Install on a clean machine. Docs match behaviour. Release only on the user's approval |

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
