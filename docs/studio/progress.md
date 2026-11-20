# Studio progress

## Ux: finishing a studio clears its windows

- Done: `studio.finished` releases every role and the stop path kills the
  terminal, but the core can refuse the `terminal.rm` that follows while the
  kill is still landing — the record outlives its program as `finished`, and
  every client keeps drawing the dead pane (`new`, `cast · <id>`, `producer`
  with "Terminal unavailable") until each is closed by hand. The stage now
  sweeps them: `reconcile::finish_sweep` returns one
  `Action::RemoveFinished` per record carrying the finished studio's own
  `backlot.studio` label, and `act` issues a bare `terminal.rm` — no kill,
  no ledger event. A `live` or `not_found` answer is the tolerated refusal
  the next tick retries; a dead wire stays fatal and a core without the
  method keeps the record. A `backlot.shell` is the user's own and a
  finished studio keeps its shells, so shells stay; another studio's record
  and a hand-named `bl-…` never match the label and are untouched. The
  studio stays in the document as `finished` and a reopened person still
  resumes its session, so cleanup costs the reopen path nothing.
  `terminal.list` includes finished records only while some studio has
  finished, so an idle core is not listing its tombstones every second.
- Measured: 4 pure `finish_tests` cover the sweep's selection — a finished
  studio's own records removed, an unfinished studio sweeping nothing,
  another studio's record, a stranger's and a shell left alone, and the
  studio still listed and still reopening into its recorded session; 4
  tick-level `finish_sweep_tests` on a duplex fake core cover the removal
  reaching `terminal.rm`, a refused (`live`) removal retried without a
  studio error, an already-gone (`not_found`) record, and a core without
  the method dialing nothing. `cargo fmt --check`, `cargo clippy
  --all-targets -- -D warnings` and `cargo test` pass; the one failing
  hc-tui clipboard test fails identically on the unmodified base commit
  (this machine has `wl-paste`/`xclip` installed where the test's stub PATH
  also reaches `/usr/bin`) and is unrelated to this change.
- Not verified: no real-core run — the mechanism (`terminal.rm` on a
  finished record) is covered by the fake core only, and the exact race
  that leaves the record was reproduced from the core's documented
  `live`/`not_found` answers rather than measured on a live session.

## P5: a studio binds a path — repository, workspace or plain directory (D4, D10, D19)

- Done: `studio new --repo` names a filesystem path, not necessarily a
  repository. The bound path classifies at pick time and again at launch —
  a git repository (zero commits is fine, an empty initial commit lands at
  launch), a workspace (not itself a repository; its direct-child git
  repositories are recorded as `members` on `studio.created` and
  `StudioConfig`), or a plain directory. A plain directory needs the choice
  spelled out — `--init` runs `git init` plus an empty initial commit in it,
  `--no-git` binds it degraded — and the flags are refused on repositories
  and workspaces. No synthetic parent repository is ever created.
- Done (workspace work areas, D10): `work/integration/<member>` and
  `work/<task>/<member>` hold one worktree per member on that member's own
  `backlot/<studio>/…` branch; the role's cwd is the container directory, so
  members sit side by side and a commit lands in the member it was made in.
  A no-git studio shares the bound path between all roles — no `work/` tree,
  no integration branch, the dependency merge check skipped. `deliver
  --commits` takes comma-separated ranges: a bare `a..b` resolves to the one
  member whose task branch holds the tip, `member:a..b` pins it, the ledger
  stores `member:sha..sha`, and an unresolvable or ambiguous part refuses
  with the pin named. Declared `[[checks]]` stay a single-repository feature
  and are refused on workspace/no-git studios at `studio new`.
- Done (pickers): `repos --json` `dir` entries carry their member repository
  names and a workspace studio marks each member `in_use_studio`. The
  wizard's terminal picker binds every readable directory — Enter picks the
  highlighted row, a trailing `/` keeps Enter on navigation — with the
  row-label vocabulary the design names (`0 commits · will create initial
  commit`, `workspace · N repos`, `directory · will git init`); a plain
  answer defaults to `--init`. The New Studio dialog lists and picks every
  shape, step 4's launch plan restates it (`workspace over N repos: a, b`,
  `will run: git init && git commit --allow-empty -m "chore: initial commit"
  in <path>`) and only there does a plain-directory pick offer the
  `no git (reduced isolation)` toggle (`g`), whose flip re-runs the dry run
  with `--no-git`/`--init`. The same argv rides `RemoteRunner` on attached
  machines — local and remote identical.
- Done (Hollywood): the studio-new card carries `no_git` through proposal
  parsing, validation and application; `draft::check` is the single shape
  gate every entry point shares.
- Measured: stage tests cover path classification and flag rules, plain-dir
  init and empty-member initialization, workspace per-member worktrees with
  commits landing in the member repository, no-git shared cwd, shape-aware
  dry runs, declared-checks refusal, member `in_use` marking and member
  ranges; picker tests cover pickable shapes, the trailing-slash rule and
  the label vocabulary; hc-tui tests cover empty-repo pickability, plain-dir
  `--init`/`--no-git` argv on `g`, and flag-free workspace argv. The full
  suite stays green.
- Not verified: `deliver --commits` member resolution is covered by unit
  tests, not by an end-to-end workspace run; no real studio has worked a
  multi-repository task yet.

## P4b: factual casting advisor and experimental production mode (L3, R14)

- Done (L3): `backlot cast --task <id> --suggest [--json]` joins the studio's
  `[[candidates]]` against the cross-studio `ActorRecord` history for the task's
  `task_kind`. The model's `casting` module matches each candidate's exact identity
  first, then the same `(runner, model, effort, permission_profile)` across runner
  versions — recorded `version_matched: across_versions` and printed
  `[matched across runner versions]`. Output is factual counts only (tasks,
  deliveries, first-delivery outcomes, defects, checks, upstream changes,
  takeovers, final outcomes, wall time, usage), `(not enough data: n/5)` under
  `MIN_TASKS`, `(no history for '<kind>')` when empty, and a dim `other kinds:`
  line when exact-kind history is thin — declaration order, no score, no rank,
  no pick (D8). `--json` prints the structured `CandidateHistory` view; it
  serializes no score field. `backlot records --kind <task_kind>` filters the
  record view by kind, answering plainly when nothing delivered on it.
- Done (R14): `mode = "experimental"` is wired end to end. `studio new --mode
  standard|experimental` parses through `correct::parse_mode`, overrides the
  cast file's `mode`, and refuses a bad value as usage; the `--interactive`
  wizard asks `mode [standard|experimental]` (Enter → standard, a bad answer
  re-asks, `--mode` skips the question) and drafts `mode = "experimental"`;
  the New Studio dialog's step 2 carries a `standard|experimental` toggle.
  `status` prints `[experimental]` on an experimental studio's header and the
  dock/sheet draw an `exp` badge — standard studios and `--plain` unchanged.
  Each role's prompt gains a research appendix only under the mode (producer:
  questions, hypotheses, criteria, kill/pivot conditions; writer: baselines and
  falsifiable criteria; director: competing variants, `cast --suggest`,
  documented negative findings; actor: isolated variants and honest
  measurements). Validation lets an experimental studio create a task with
  `requirement_version` 0 — direct-to-task exploration without a formal spec —
  while standard keeps requiring a recorded version.
- Measured: model tests cover exact/cross-version matching, merged records,
  thin-data and no-history notices, declaration order and the no-score JSON
  shape, plus experimental `requirement_version` 0 acceptance and standard
  refusal; stage tests cover the suggest view text and JSON, the `--actor`
  usage error, unknown-task refusal, `records --kind`, `--mode` flag/TOML/CLI
  precedence, wizard mode selection and re-prompt, the status badge, and the
  direct-to-task allowance; hc-tui tests cover the `exp` badge on dock and
  sheet and the mode toggle.
- Not verified: no real agent has run an experimental studio or produced a
  record a suggestion read — every history in the tests is seeded.

## P4a: ledger schema versioning, boundary hardening, usage & cost (2026-11-21)

- Done: every studio and Hollywood event envelope carries `v: 1`
  (`backlot_model::LEDGER_VERSION`); a line without `v` deserializes as `v: 1`, and a `v`
  the binary does not know is refused with "ledger event version … is newer than
  supported …; upgrade backlot" instead of a serde failure. `migrate.rs` is a pure,
  I/O-free pipeline — `migrate_event_value` / `migrate_hollywood_value` on
  `serde_json::Value` — that stamps `v: 1` on unversioned lines and loops `v → v + 1`
  through a step table built for future versions; the store's `parse_log` passes every
  line through it, so old logs load transparently. `backlot studio migrate
  [<studio> | --all] [--dry-run]` rewrites a log under `events.lock` — temp file,
  fsync, atomic rename, `events.jsonl.v0.bak` kept, the migrated log replay-verified —
  prints `already at ledger v1` on a no-op, and `--all` includes Hollywood's log.
  Working-role callers are refused.
- Done: D17a boundaries hardened. The permission-prompt spoofing check in `permit.rs`
  now flags a block reaching for `BACKLOT_` reads/`unset`/`export`/`declare`,
  `env -u`/`--unset`, `/proc/*/environ` reads and `env`/`printenv` dumps coupled with
  secret or `.env` paths — on top of the existing `env -i`/`--ignore-environment` —
  while ordinary commands (`cargo test`, `backlot deliver`) still owe the director the
  prompt. `report show --file` rejects NUL bytes on top of the existing
  absolute/`..`/symlink containment; studio and card identifiers are validated before
  any path is built (`studio_state`, `hcard`, `resolve_draft` `..` escapes, leading
  `-`), and `run_action` refuses argv or machine names carrying control bytes so a
  hostile `backlot` command line is never spawned.
- Done: `task.delivered` carries an optional `usage` — `input_tokens`,
  `output_tokens`, `cache_read_tokens`, `cost_micros_usd` (integer micro-dollars:
  exact JSON, exact `Eq`) — reported by the runner through `backlot deliver
  --tokens-in|--tokens-out|--cache-read-tokens|--cost-usd`. The model sums it
  factually: `StudioState::usage` across deliveries, `ActorRecord::usage` per actor,
  `StudioSummary::usage` only while non-zero (older `backlot/1` documents stay
  byte-identical). `status` prints `usage: 12.4k in / 3.1k out / 2.0k cache · $0.08`
  under the limits, `records` appends `· usage …` to an actor's line, and the dock
  draws `∑ 12.4k/3.1k tok · $0.08` under the owed line plus a `usage …` row on the
  sheet — all only while a counter moved; `--plain` is untouched. No score anywhere
  (D8).
- Measured: model tests cover serde round-trips, `v` defaults, unsupported `v: 0` and
  `v: 99`, a synthetic three-step migration, `UsageTotals` accumulation, formatting and
  summary omission while zero; stage tests cover `studio migrate` (dry-run, real
  migration with backup and state equality, no-op, future-version and working-role
  refusals), the spoofing vectors against benign-command non-regressions, NUL/
  traversal/symlink refusals, hostile identifiers and argvs, and the `deliver` usage
  flags (exact micros, malformed/negative refused) with `status`/`records` rendering;
  hc-tui tests cover the argv control-byte refusal and the usage row drawn only when
  present.
- Not verified: no real runner has reported usage — every figure in tests is a
  fixture — and no real log older than `v: 1` has been migrated on a live state dir;
  the migration path ran on synthetic logs only.

## P2b: rate-limit and connection-error detection, stage auto-restart (2026-11-20)

- Done: `ratelimit::limited` reads a reporting core's fresh screen tail whenever its status is
  not `working` — so Devin's rate limit (`done`, `Reached free model rate limit … reset in 54
  minutes`) and Claude Code's session/weekly/monthly screens are observed — while `working`
  still never matches and an `error` keeps its message-then-screen order. `hit your limit`,
  `session limit`, `weekly limit` and `monthly limit` joined the vocabulary, and the result is a
  `LimitHit { wait_ms, escalate_now }`: a weekly or monthly quota raises `rate limited` with
  `needs the user` at once, records the `rate_pace` sentinel so a cleared attention never
  re-raises on the same banner, and clears `resumed` when it goes. Short limits keep the parsed
  reset, margin and three `continue`s. The unit tests moved to `ratelimit_tests.rs`.
- Done: `connerr.rs` recognizes Devin's `Connection error` in an `error` message or a
  non-working fresh tail (never a `working` screen; a rate limit on the same sample wins),
  raises its own `connection error` attention, sends `continue` after 15 seconds, escalates
  `needs the user` after three, and clears `resumed` when the banner goes. Its `Stage` pass
  keeps `conn_pace` under the same adjudication invariant as `rate_pace`; `limits.rs` suppresses
  `agent error` and clears it `connection error observed` when this pass claims the sample. The
  stage's stall pass moved into `stall.rs` to keep `limits.rs` compact.
- Done: `supervise.rs` is a pure, clock-injected `RestartPolicy` — at most five starts in ten
  minutes, backoff 2→4→8→16→30 s, a stable minute resets it, and the spent budget pauses once
  per episode until an attempt leaves the window or the stage returns. `stage watch --restart`
  starts a dead `backlot-stage` through the extracted `stage::start_on` (the same idempotent path
  `stage start` uses), reconnects a dead core with a 2→5→15→30 s backoff (`core disconnected,
  reconnecting` / `core reconnected`, once per edge), and checks the stage immediately after.
  Without `--restart` the watch behaves as before. `service.rs` renders and optionally writes
  the systemd `--user` unit (`backlot-stage[-<session>].service`, `Type=simple`,
  `ExecStart=… stage watch --restart`, `Restart=on-failure`, `RestartSec=5`, `PATH` and
  `BACKLOT_STATE_DIR`), refuses `--print --write`, `--remote` and working-role callers, and
  never runs `systemctl`.
- Measured: 639 `backlot-stage` lib tests, including `ratelimit_tests` (reporting core at
  `done`/`idle`/`waiting`; working ignored; stale scrollback ignored; weekly/monthly
  `escalate_now`; the existing reset-parser cases kept intact), `connerr_tests` (sample rules,
  both-words precedence, the full retry → escalate → `resumed` cycle for both taxonomies on a
  fake core with an injected clock, and the weekly sentinel), `supervise_tests` (backoff
  progression, 5-in-10m pause and recovery, pause ended by a live stage, stable-live reset, the
  new parser forms and refusals, working-role refusal) and `service_tests` (unit contents,
  session unit, ExecStart quoting, state-dir passthrough, unsafe session rejection, `--write`
  inside a scratch `XDG_CONFIG_HOME` with the stdout commands, `--print` writing nothing).
- Not verified: no real agent has hit these paths since they were written — every limit and
  connection-error screen is a fixture, and the retry cycles ran on a fake core only. The
  systemd unit has never been loaded by a real user manager, and the reconnect path has not met
  a real `hc` restart. The watch's own loop was not driven against a scratch core; the policy,
  the wire-death verdict and the parser are what the tests cover.

## U6c: per-machine backlot versions, install/upgrade and stage restart (2026-11-20)

- Done: `Summary` carries an optional `backlot_version` (`backlot/1` stays — older documents
  deserialize `None`); the stage publishes its own `CARGO_PKG_VERSION` on every publish, and
  `same_as` counts a version change as real so an upgraded stage republishes once.
- Done: `machine_versions.rs` is the per-machine fact — the document's `backlot_version` wins,
  then a probe verdict, then `Probing`/`Unknown`; probes run `backlot --version` through
  `terminal.run on: <machine>` (the RemoteRunner's locate script — `command -v`, a login shell,
  `~/.local/bin`), are cached for six hours, re-fire on connect and on an offline → online
  return, and never touch an offline machine. The local machine answers `env!()` +
  `current_exe` directly.
- Done: the expanded sidebar's endpoint rows grow a second line in studio mode —
  `hc <endpoint v>` dim ahead of `backlot <v>` / accent `↑<latest>` / `no backlot` / dim
  unknown — `hc` elides first at narrow widths; `--plain` never builds the lines, so Herdr's
  one-line tree is byte-identical.
- Done: `MenuKind::Machine` — right-click or the `Ctrl+B ?` `machines` group — lists only the
  valid rows: install when `Missing`, `Upgrade backlot to v<latest>` when behind, `Start`/
  `Restart the stage` on the stage's state, `New studio here` when installed, then the endpoint
  ops; an unreachable machine dims command rows except the endpoint repair ops.
- Done: install/upgrade probes the target's `EXE=` + env read-only first, refuses mise/
  Homebrew/Nix/cargo/dev-build binaries with the channel's own command, and otherwise opens a
  `terminal.run` popup on that machine with `curl | sh` — `BACKLOT_SKIP_CORE=1`, a
  semver-validated `BACKLOT_VERSION` pin, a shell-quoted `BACKLOT_INSTALL_DIR`. The popup's
  close invalidates both caches and re-probes; a bumped version on a stage-running machine
  chains `stage stop` → `stage start` through the action tracker. `Target::Stage` makes the
  `stage not running` rows clickable — Enter/click/`r` run `stage start` on the row's machine.
- Measured: 6 state-machine tests (6 h throttle, offline skip, doc-beats-probe, verdict parse,
  invalidation, semver compare), an endpoint-row elision test, 15 App tests (the two sidebar
  lines, the `↑` hint, `no backlot`, 80×24, `--plain` rows, the menu's five shapes, offline
  dims, right-click, stage-row click, stop→start and no-restart-on-failure, the refusal notice,
  the armed install popup and the post-close re-probe), plus `machine_install`'s probe/line
  tests — 1351 lib tests pass.
- Not verified: the installer popup's live `curl` download and a real remote host's install —
  the popup is `terminal.run`-launched like the proven studio actions but was driven only
  against the fake core; the same environmental clipboard flake from U5
  (`missing_tool_is_unavailable_not_no_image`, real `wl-paste`/`xclip`/`xsel` in `/usr/bin`)
  still fails here — pre-existing, unrelated.

## P2a: stage resilience — git timeouts, producer notices, studio prune (2026-11-19)

- Done: `gitcmd.rs` gives every stage git call a 60-second bound. `real_git` spawns git in
  its own process group, drains stdout and stderr on reader threads into capped tails, and on
  timeout sends SIGTERM to the group, waits one second, then SIGKILLs and reaps the child. The
  failure reads `git timed out after <n>s: git <argv>`, and `err_line` prefers that line, so
  `work area failed` names the timeout instead of hanging the stage.
- Done: a producer `stalled` or `cannot resume` now tells a live director once — an
  informational body saying the director cannot restart the producer and only the user or
  Hollywood can intervene — and `backlot status`/`status --all` lead the studio block with
  `producer alert: <reason> — <detail> (needs the user or Hollywood)`. Hollywood's notice for
  the same event is always `stalled`, headed `<studio>: producer stalled` or
  `producer cannot resume — needs attention`. A producer still notifies nobody when no
  director is live; the notice drops `attention cleared` when the producer recovers.
- Done: `backlot studio prune [<studio> | --all-finished] [--dry-run] [--force]` reaps a
  finished studio's `work/` worktrees and `backlot/<studio>/*` branches. A live studio exits 2.
  Without `--force` a dirty worktree is kept, clean ones go through `git worktree remove`, a
  vanished tree goes through `git worktree prune`, and branches go only through `git branch -d`
  so unmerged work is kept with git's reason. `--force` adds `worktree remove --force` and
  `branch -D`. `--dry-run` reads git and prints the plan. The ledger, `reports/`, `messages/`
  and `attention/` are never touched.
- Measured: `gitcmd_tests` — fast git keeps exact stdout/stderr/exit code; a hung git alias
  returns `git timed out after 0.2s` in under five seconds; a timed-out `sh` with a background
  `sleep` leaves no surviving grandchild; cwd is honoured. Producer routing and status:
  `stall_tests` (live vs absent director, body wording, no `tell producer` nudge),
  `notices_tests` (producer `cannot resume` to a live director), `hnotice_tests` (producer
  down stays Hollywood's `stalled` notice with the producer heading), `stale_tests` (the
  director's copy drops when the attention clears), `render_tests` (the alert leads the status
  block). Prune: five tests — safe/force/dry-run planning over a recording fake git, missing
  worktree pruning, refused remove reported as kept, foreign worktrees/branches ignored, and
  one real-git CLI run proving live-studio exit 2, dirty-worktree retention, branch deletion
  only when safe, `--force`, and ledger/reports/messages preservation.
- Not verified: no real hung git (network file system, credential helper) has been
  reproduced; the 60-second value is untested against a real slow checkout. Producer notices
  have run on the fake runner only — no real-agent stall or restart. Prune has not run on a
  real studio's worktrees; `--all-finished` is covered by the fake/CLI paths but not a
  multi-studio real tree.

## U5: a repository picker and agent detection for the wizard (2026-11-18)

- Done: `wizard_picker/` holds a repository picker — state/filter/tree/render/term behind the
  `Io.pick_repo` seam — so `studio new --interactive` on a terminal replaces the bare
  `repository:` line with a full-screen picker: typing filters live (substring then subsequence,
  repositories first, the root tracking the deepest existing directory), `Tab` completes the
  longest common prefix and descends on a single match, arrows/PgUp/PgDn/Home/End move, `→`/`←`
  expand/collapse/climb, `Enter` picks a `●` repository (branch + commit age, `git log -1` per
  visible row, cached; `○` has no commits and cannot be picked), `Esc` refocuses then cancels,
  `Ctrl+C` cancels at once. Hidden dirs stay hidden unless the filter starts with `.`;
  `node_modules`/`target`/`.git` never list; >500 children truncate with a count line; unreadable
  dirs show `(no access)`. While the input is untouched, up to eight repositories the core's
  terminals have as cwd — deduplicated to their roots — lead the tree (`wizard::in_use` over
  `terminal.list`; an unreachable core just has no group). Non-terminal stdin/stdout keeps the
  line prompt.
- Done: `runners/detect.rs` finds each runner's binary past the inherited `PATH` — login-shell
  `PATH` via `$SHELL -lc` between marker lines, then known dirs (`~/.local/bin`, nvm `*/bin`
  newest first, Volta/bun/cargo/npm-global/deno, mise/asdf shims, Homebrew, `/usr/local/bin`) —
  probes `--version` at 3 s, sanitizes its first line, and reports `needs PATH` for a find the
  inherited PATH cannot reach. `backlot runners` prints the table (runner, status, version,
  path, needs PATH) with no core; the draft shows the same per runner. Accepting the cast writes
  `[runner_env.<runner>] PATH = "<bin dir>:<inherited PATH>"` into the state root's
  `config.toml` — lines shown first, file born `0600`, an existing PATH never rewritten (a
  warning when it still cannot reach the binary) — and `draft::check` counts a configured PATH
  as installed like a launch. The draft models moved: claude `sonnet` (the CLI alias, so the
  draft tracks current), opencode `anthropic/claude-sonnet-5-5`; `gpt-5.3-codex` stays.
- Measured: 55 wizard tests (terminal pick replaces the line prompt, non-terminal keeps it, in-use
  group absent on an unreachable scratch core, PATH written once/never overwritten/warned, refused
  writes nothing), picker state/filter/tree/fs/render suites incl. 100×30 and 60×20 text
  snapshots and no-panic tiny sizes, 10 detect tests over fake dir trees and fake `$SHELL`, and a
  draft test proving a configured PATH counts as installed.
- Not verified: the picker's keys and look were exercised in a real popup only by the buffer
  snapshots, not a live TTY; the login-shell probe is untested against a real nvm login; the
  `runner_env` PATH write is proven against `draft::check` and the config parser, not a launched
  role. One environmental flake seen: `clipboard::tests::missing_tool_is_unavailable_not_no_image`
  fails on hosts with real `wl-paste`/`xclip`/`xsel` in `/usr/bin` (its stub PATH includes it) —
  unrelated to this diff; the suite was green on the gating runs.


## S14b: the cline adapter (2026-11-17)

- Done: `runners/cline.rs` plans and preps a Cline CLI 3.0.74 role — `cline -i --yolo -P cline-pass -m
  <model> [--thinking <effort>] [prompt]`, `--yolo` on every argv because hub mode would run the
  actor's commands in the hub daemon's environment, where `BACKLOT_*` and `PATH` never reach them.
  `--id <id>` resumes with no positional prompt. A model outside `cline-pass/` is a `model not
  supported` refusal (no config channel carries a provider), and an effort outside `none`, `low`,
  `medium`, `high` and `xhigh` is an `effort not supported` one. `runners/cline_hook.rs` generates
  the permission layer: a POSIX `sh`
  `<cwd>/.cline/hooks/PreToolUse.sh` (mode 0755, no `jq` — nothing here assumes it), default-deny
  against the role's command rules, the network tools, MCP names, secret reads and writes outside
  the work area, with one bounded line per decision in the role's signals file. The prep also writes
  `roles/<role>/cline-mcp.json` (`{"mcpServers":{}}`, named by `CLINE_MCP_SETTINGS_PATH`) and keeps
  `/.cline/` in the worktree's `info/exclude`. Session discovery reads the manifest under the user's
  session store (cwd, start time, and the pid that is the engine child of the terminal's `node`),
  and the work states, prompt block and approval policy follow S14a's captures.
- Measured: 34 adapter tests — the exact argv shapes with `--yolo` in each, the never-flags list, the
  provider and effort refusals, the generated hook run on nine sample tool calls (an allowed git
  command, a `curl`, `web_search`, an MCP name, a write outside the worktree, a secret read, a chain,
  a nameless call, a long target), the prep's files and modes, the exclude line (plain and
  `gitdir` worktrees), the manifest lookup with wrong cwd, older start, no manifest and a pid
  mismatch, plus the neutralized screen fixtures for idle, working and the approval block.
- Not verified: no real `cline` launch — every test is fake or fixture. Unproven until the real run:
  whether the hook is re-read mid-session, whether the hook's `cancel` verdict behaves exactly as
  the measured print (it ends the turn), the rate-limit wording (the placeholder list is empty on
  purpose), and whether the manifest's `pid` is the engine process on every node version.


## U2: studio first — every terminal belongs to a studio, fake runner only (2026-11-16)

- Done: `backlot` is a studio tool and nothing else. `backlot --plain` keeps the plain
  multiplexer exactly as on `main` — the parity harness runs it now. Without `--plain` the
  client is always in studio mode, document or not: the cast sidebar shows the no-studio view
  (header, `others`) until a studio exists. With no studio the **opening page** covers the pane
  area — what backlot is in one line, then `n` (a popup running `backlot studio new
  --interactive`), `h` (Hollywood's input line, only with `[hollywood]`), `s` (start the stage,
  only while it is down), `q` detach, and a dim line naming `backlot --plain`; `Esc` hides it
  for the session, `new_workspace` brings it back. In studio mode `new_tab` and split create a
  **studio shell**: the user's shell in the focused person's workarea (an actor's `work/<task>`,
  the director's `work/integration`, else the studio repo), named `bl-<studio>-sh<n>` (a form no
  role or task id can take), labelled `backlot.studio`+`backlot.shell` plus the studio's space
  and a `shell <n>` tab. The cast groups them under `shells` after the people, the role keys
  reach them after the people, the client's reaper treats them like ordinary terminals
  (`backlot.studio` protection exempts `backlot.shell`), and the stage never messages, nudges,
  samples, counts, reaps or relabels them — `studio_view` excludes them wholesale. The stage
  launches roles with `herdr.space=<studio>` and `herdr.tab=<person>` (an actor's task appended:
  `actor-b t2`) instead of one shared `backlot` space; terminals already live when a stage
  starts are relabelled on its next reconcile — idempotent, ledger-only, skipped where the core
  cannot change labels (`CallError::Unsupported` logs once). `others` holds only what is neither
  a person, a shell, the stage nor Hollywood. The summary keeps schema `backlot/1` with two new
  optional fields: `repo` on the studio and `workdir` on each role (the daemon fills it from the
  work-area table). `backlot studio new --interactive` asks on the terminal for what the flags
  leave out — repository (the cwd when it is a committed git repo), studio id (the repo's
  directory name), title, cast (drafted from installed runners in `devin, claude, codex, cursor,
  opencode, cmd, fake` preference order, shown, `accept`/`edit`/`refuse`, `$VISUAL`/`$EDITOR` on
  a pid-unique temporary file, `draft::check` + the ordinary event path on accept, the stage
  offered after) — and writes nothing before the final accept; `--dry-run` with it is a usage
  error.
- Measured: unit/draw tests across `hc-tui` and `backlot-stage` (907 + 462 + the workspace's
  other suites; all green). The `--interactive` wizard's scripted-stdin tests cover every
  prompt, the defaults, an edited cast, a refused cast and each `studio new` refusal. A real
  PTY run on the pinned `herdr-core` (`real_u2_studio_first`, `fake` only): opening page at
  200/120/80/40, `n` driving the wizard end to end (repo → id → title → draft accept → stage
  `y`), the cast and the studio's own space appearing, `prefix+c` opening `bl-acme-shop-sh1` in
  the actor's worktree under `shells`, and `backlot --plain` on the same core showing the plain
  agents list; captures under `backlot-work/briefs/U2-screens/`.
- Not verified: a real-agent studio under U2; the relabel pass against official `hc` (the
  `Unsupported` back-off is unit-tested only); shells on a remote endpoint's machine.

## H4: Hollywood — the floating layer in the TUI, fake runner only (2026-11-16)

- Done: `src/hollywood/` (model, rows, draw, keys, actions, the read-only card view) and
  `src/app/hollywood/` give the client a per-client layer with three states — hidden,
  a collapsed strip at the top row's right end (`✦ hollywood live · 2 live · 1 card ·
  <headline>`, content-sized, capped at a third of the width) and an expanded panel
  dropping from the top (capped at half the height, scrollable, full-screen at narrow
  widths). It draws over the panes, the cast sidebar and the studio panel and reserves
  nothing — no resize, ever. `alt+y` (direct, no prefix) cycles the states only while
  `[hollywood]` exists; without the table it reaches the program untouched and no timer
  runs. `[hollywood] key` rebinds it — direct or `prefix+<key>` chord — with parse,
  bare-printable and studio-key/panel-chord collisions falling back to `alt+y` under a
  file-naming notice that never echoes the value. Expanded owns its keys like the
  focused panel: j/k/arrows/PageUp/PageDown, Enter activates (studio row → producer,
  title → `bl-hollywood`, card → Show me, input → edit), `a`/`c`/`s` apply/cancel/show
  the selected card, `i` edits the `tell hollywood` line, `x` hides, `Esc` collapses.
  Mouse: strip click expands, rows/buttons/input/scrollbar/wheel all hit, an outside
  click falls through and leaves the panel open. Show me on a `studio-new` card opens
  `backlot card show` output in a scrollable read-only view; on another card it focuses
  the studio. Actions spawn `backlot` as the user off the event loop through an
  injectable runner; failures notice. All content is the summary document plus the
  stage flag — studio rows read even with Hollywood down, and a stopped stage gets its
  own line. `backlot hollywood setup` writes/replaces the `[hollywood]` table atomically,
  keeps the rest of the file byte for byte and refuses every bad input before writing;
  the expanded studio panel's last row names the command while the table is absent.
  `backlot card show <id>` prints a card's full read — a `studio-new`'s draft cast and
  brief included. A running stage picks the table up on its next tick; a client at its
  next start — no file watcher.
- Measured: 22 `hollywood::tests` (rows/keys/draw at 200/120/80/40, both themes) and 28
  `app::tests::hollywood` (keys, actions, mouse, no-resize, absent parity, the setup
  hint); 6 `hsetup_tests` on the stage side. `real_h4_hollywood_layer` on the pinned
  `herdr-core`: the strip with the headline at all four widths, the expanded rows with
  both studios and card `c3`, `j` + `a` applying it to `card.done` (`by: user`, the
  stage's `card c3 done` log visible in the pane), the input line's `hold the line`
  delivered to Hollywood's inbox (`message.sent` + `message.delivered`), `x` hiding
  everything; captures under `backlot-work/briefs/H4-screens/`. Parity: the harness's
  config carries no `[hollywood]` — this tree's run and a clean `9aa0660` build's run
  are byte-identical signature runs to the recorded U1 logs apart from worktree paths
  (79 checkpoints, the same `popup-open` environmental timeout; the navigator
  checkpoints' ±1 style-cell count is a run-to-run flake — the same base binary showed
  19 on U1's recorded run and 18 on this one, all blank cells in the known
  theme-delta region). `without_the_table_alt_y_reaches_the_pane_and_no_cell_changes`
  proves the key and the cells. Gates all green.
- Not verified: a real agent in `bl-hollywood` (H5); `hollywood setup` on an installed
  release (the runner-installation probes are argv-only); the chord form of
  `[hollywood] key` on a real terminal (unit-covered only); the layer under a remote
  core (the panel's `local_documents` limit applies to it alike).

## U1: the studio layout — cast, blocks and Alt-arrow navigation (2026-11-16)

- Done: studio mode, latched while the last good `backlot/1` document lists a studio —
  a missing, malformed or foreign document and a stopped stage keep the mode; only a
  good document with zero studios leaves it. The sidebar's agents section becomes the
  current studio's **cast** (`src/cast/`): header `cast · <id>` + `<n>/<total>`, two rows
  per person (mark, name and task with the runner at the right; the terminal's title —
  `doing`, then the state word as fallbacks — with a short model), `! <attention>` or
  `last turn failed` second rows, the focused person's `▌` active rows, closed people
  last and dimmed, and an `others <n> ▸` row expanding herdr's agent rows for the
  unseated. The spaces section caps at a third of the sidebar height; the cast's wheel,
  scrollbar and click hits are its own. The panel is a narrow column (`[panel] width`
  default 26, was 30, clamp 22–60) of bordered studio blocks — `╭ <id> ── !<n> ╮` in the
  accent for the current studio, `├──┤` rules between the role groups, one line per
  person with the state word right-aligned and an attention line under it — with
  stacking (`╶ <id> ⠿2 ○3 ×1 !<n>`) when the blocks do not fit, a dim shortcuts line
  (dropped under 8 rows) and a per-studio line in the collapsed strip. Four direct keys
  (`[keys] previous_studio`/`next_studio`/`previous_role`/`next_role`, alt+arrows by
  default) move between studios in document order and between the current cast's live
  people through the existing focus path — no pane resizes — landing on each studio's
  remembered person. They win over herdr bindings in studio mode only, fire while the
  panel or a persistent float is focused (the float parks), go to the program while the
  keyboard is locked, and unbind on `""` or fall back with a file-naming notice on a bad
  value. `Space` toggles a block's collapse; a block's top border and a stacked line are
  click targets. New `[sidebar] width` (26, clamp 20–60) applies in studio mode; a drag
  still overrides it for the session. No lines changed outside `hc-tui`.
- Measured: ~70 new or rewritten tests — `cargo test --workspace` 1798 passed across 75
  binaries, 826 of them hc-tui's; `cargo fmt --all -- --check` and
  `cargo clippy --workspace --all-targets -- -D warnings` clean. Real-core:
  `real_u1_studio_layout_screens` on the pinned `herdr-core` — one scratch core, two
  studios, the fake runner, a `work:blocked` actor — captures the cast, the bordered
  blocks, stacking at short height, the `!1`/`! permission prompt` attention on both
  surfaces and Alt+down/up/right switching at widths 200, 120, 80 and 40 (mobile chrome,
  no sidebar, as designed under 65 columns); the captures sit in
  `backlot-work/briefs/U1-screens/`. The ignored real-core suite passes beside it
  (`integration` 5, `real_core_smoke` 1, `real_stage` 22, `real_two_studios` 1). Parity
  outside studio mode: this tree and a branch-point (`cce69a3`) build produce
  byte-identical parity logs against the herdr reference — 79 checkpoints, the constant
  7-cell backlot signature, the same environmental `popup-open` timeout the recorded S13
  run hit — and `outside_studio_mode_the_herdr_binding_gets_the_key` proves `alt+left`
  reaches `previous_tab` with no studio.
- Not verified: a real runner under the layout; three or more studios; the `others` row
  on a long agent list. Entering or leaving studio mode can resize the panes once — the
  sidebar width switches between herdr's `ui.sidebar_width` and `[sidebar] width`.

## H3: Hollywood — the stage, fake runner only (2026-11-16)

- Done: with `[hollywood]` in the user config the stage runs one terminal `bl-hollywood`
  (labels `backlot.role=hollywood`, no `backlot.studio`) in `<state dir>/hollywood/` —
  `AGENTS.md` rewritten each start, `memory.md` made once, `messages/`/`reports/`/`drafts/`
  created. The lifecycle is H1's model plus one event kind, `role.state`, folding the studios'
  four role events into one record (`started`/`lost`/`resumed`/`closed`, terminal, actor,
  session, reason — `stage` only); start, adopt, rejoin, lost, resume, the three-losses-in-
  ten-minutes `cannot resume`, close-on-config-change and stray drop all go through `hplan`'s
  pure table. The launch pre-allows exactly `status`, `records`, `tell <studio> producer`,
  `hollywood propose`, `hollywood report` and `studio new --dry-run` on each adapter (claude,
  cursor, cmd); the stage never answers a prompt there and owes it no steps. `tell hollywood`
  delivers through the same guarded send the studios get, and the stage's notices — attention,
  user-only prompt, finish, failure, accepted delivery, unrouted stall, recovered terminal,
  every card outcome — queue as `message.sent` with `source_seq` (or the error text for a
  source-less `failed`), dedup on it across restarts, and drop `message.dropped` when the
  source moves on. The card pass executes an applied card once: base re-check (`card.stale`
  and nothing else), the studio event `by: user` through the same validation, `card.done` or
  `card.failed` with the reason; an effect already in the ledger (the `card c<n>` marker, the
  priority, the `purpose: "brief"` message) makes a restart repeat nothing. `studio-new`
  re-runs the dry-run checks, copies the cast, writes `studio.created` and queues the brief
  `by: stage`. Cards run with or without `[hollywood]`. The summary document carries the
  optional `hollywood` object — role state, runner, model, headline, open cards.
- Measured: the 18 model tests now cover `role.state` folding and validation, `applied_seq`/
  `closed_seq`, the tightened outcome rules (`card.stale` is the user's on an open card, the
  stage's on an applied one) and `source_seq` on `message.sent`. 21 new stage tests cover
  `hplan`'s truth table and env, every owed/dedup/stale notice row and `hnotice`'s
  label shaping, and `hcard`'s execution — done/stale/failed, idempotent restart, the
  studio-new crash window. The real-core suite `real_hollywood` (2 tests, fake runner,
  scratch core): two studios overseen, a notice from each naming it, `tell hollywood`
  delivered (through a digest), the summary object, a proposed+applied pause card carried out
  as `task.state` `by: user` plus `card.done`, a stage-only restart that adopts nothing and
  loses nothing, a moved-base card `card.stale` with no studio event, a `studio-new` card
  that lands `studio.created` and the producer's brief in the fake inbox, and the no-config
  run where none of it exists. Run 5 times in a row on the pinned `herdr-core`: ~31 s each,
  all green; `cargo fmt --check`, `clippy -D warnings` and `cargo test --workspace` (1626)
  pass beside it.
- Not verified: a real runner has never launched `bl-hollywood` — the pre-allow lists are
  wired per adapter but only `fake` ran them, and a real overseer answering nothing but
  status/propose/report is untried (H5). The TUI layer does not exist (H4): `alt+y`, the
  collapsed strip and the expanded panel are design only. Card outcomes reach Hollywood's
  inbox but no agent has yet read one and replied.

## H2: two live studios on one core, fake runner only (2026-11-16)

- Done: `real_two_studios_side_by_side` under `crates/hc-tui/tests/` — one scratch core, one
  stage, two studios on their own scratch repositories: `acme-shop` and `acme`, names that
  share a prefix, both carrying task `t1` and the same role names (`producer`, `actor-a` →
  `actor-t1-c1`) under `[limits] fake = 3` casts. The run interleaves their whole
  productions — requirement, plan request and publish, dispatch, `task new`, `cast`, a
  `tell` to each producer, a permission prompt on each actor, `deliver`, the declared
  check, `review`, merge to integration and `studio.finished` — with a stage-only restart
  in the middle of it. The shared harness it drives moved out of `real_stage.rs` into
  `tests/common/real_kit.rs` studio-parameterized (`bl`/`ledger`/`wait_*` take the studio),
  so the file did not grow; the fake runner now writes `fake-inbox-<studio>-<role>` beside
  the legacy `fake-inbox-<role>`, since a shared state dir would collide the role key.
  No bug in `backlot-stage` or `backlot-model` surfaced — the test is the deliverable.
- Measured: terminal ownership asserted over the whole inventory — every `bl-` terminal's
  `backlot.studio` label matches a `bl-<studio>-` name and the exact expected set, nothing
  shared or borrowed — six live `fake` roles over two `fake = 3` caps prove the cap counts
  per studio. Each ledger held only its own events: `role.started` session ids all
  `fake-bl-<studio>-*`, `task.delivered` its own repo's `b1..b2`/`a1..a2`, every
  `message.delivered` targeting a seq its own ledger queued. Each `tell` reached one
  producer's studio-keyed inbox and one `messages/` dir only. With both actors blocked at
  once, `acme`'s delivery and `units` check ran while `acme-shop`'s prompt still held —
  `task.checked`'s log shows the delivery's own end sha and repo — and `approve` sent
  `enter` to one actor's terminal, `esc` to the other's, each screen clean of the other's
  key. `acme-shop`'s `studio finish` was refused while its `t1` was open, then closed its
  roles and finished while `acme` stayed `live`; `acme` finished after. The stage restart
  adopted all six terminals — same addresses, no `role.lost`/`role.resumed` in either
  ledger. `status --all` and the document showed one `finished`, one `live`, then both
  `finished`. Run 7 times on the pinned `herdr-core` (5 in a row): ~29 s each, all green;
  the whole ignored real-core suite — `integration` 5, `real_core_smoke` 1, `real_stage`
  22 — passes beside it, plus `cargo fmt --check`, `clippy -D warnings` and
  `cargo test --workspace` (1605).
- Not verified: real agents in two studios — the fake runner is the only runner that has
  run, and its studio-keyed inbox is the test's own seam. A third live studio, two studios
  on real-runner caps, and the panel drawn on two live studios are all untried.

## H1: Hollywood — model and CLI, fake runner only (2026-11-16)

- Done: `backlot_model::hollywood` holds the overseer's own ledger model — `card.proposed`,
  `card.applied`, `card.cancelled`, `card.stale`, `card.done`, `card.failed`,
  `hollywood.reported` and `message.sent` in the studio envelope — with its reducer (open and
  closed cards, capped headlines, messages), its strict append-time validation (who may write
  which kind, action/base coherence), and `stale`, the pure base check. The stage appends it at
  `<state dir>/hollywood/events.jsonl` beside `studios/`, never inside one, and `hollywood` is a
  reserved studio name. `roles::check` lets Hollywood record only a plain `tell` message to a
  studio's producer (R26); the summary document's optional `hollywood` object (200-char
  headline, 10 open cards) stays inside `backlot/1` and under 64 KiB. The CLI:
  `hollywood propose <action>` for every D18 action (`studio-new` accepts only a draft that
  passes the shared `studio new --dry-run` check — name, reserved name, taken name, git repo,
  cast and check declarations, supported and installed runners, caps that seat the fixed
  roles), `hollywood report`, `card list|apply|cancel` (apply records `card.applied` or
  `card.stale` and stops — execution is H3), `tell hollywood`, `status --all` and
  `records --all`/`--studio`, and the `[hollywood]` config section (`runner`, `model`, `effort`,
  `key`, `default_cast`, secret-safe parse errors). A Hollywood `tell producer` mirrors
  `message.sent` into its own ledger. Docs: R21–R27 and D13–D21 in requirements.md,
  `hollywood.md`, the ledger's Hollywood section, and H1–H5 in plan.md.
- Measured: 18 model tests cover the envelope's wire shape, replay-equals-apply, every
  card-state transition and staleness row, every role rule accepted and refused, the summary's
  back-compat and 64 KiB cap. 21 stage CLI tests cover the ledger path and independent seqs,
  propose's base checks, the card gate (apply/cancel refused under `BACKLOT_ROLE`, list open),
  `tell hollywood` and its role refusals, the producer mirror, the cross-studio reads, the
  reserved name, and each `--dry-run` refusal kind with nothing written. 5 config tests cover
  the section, its runner/effort rules and position-only errors.
- Not verified: no stage loop, terminal or layer exists — `card apply` records the intent only,
  `tell hollywood` lands in the ledger undelivered, and `[hollywood]` is read, not launched.
  The card gate is a working rule (D17a), not yet backed by a pre-allowed launch (H3). No real
  runner and no live studios; H2 and H3 carry the behaviour.

## S12: the cmd adapter and two mixed-cast real runs (2026-11-13, 2026-11-14)

- Done: `runners/cmd.rs` launches Command Code (`cmd`) for a role, with its work states, its
  prompt block and a per-worktree `.commandcode/settings.json` (mode 600) that pre-allows
  `backlot`, the actor's git subcommands and the declared check. `effort` is optional in the
  cast: `cursor`, `devin` and `cmd` take none. Work detection reads the structured status
  a terminal reports first and the screen second. Session discovery has a window per runner
  (cmd 240 s). New commands: `backlot attention clear <seq>` and `backlot stage watch`. A rate
  limit is told apart from other agent errors and retried a bounded number of times.
- Run (S12c): Devin as director, cursor-agent as writer, one cmd actor, the producer on `fake`.
  The loop completed up to the user's `finish`: 83 ledger events, one task delivered, checked
  by the stage, accepted and merged. The director answered the actor's four prompts.
- Run (S12e): the same cast with two cmd actors at once, 323 ledger events in 1 h 48 min, to
  `studio.finished`. 86 attentions raised and 86 cleared. Devin and cmd were read from their
  structured status, cursor from its screen.
- Found: cursor's `Write to this file?` picker was not a prompt to the stage in either run and
  was answered by hand; a role closed by `studio.finished` could keep an open attention. Both
  fixed afterwards (S12f): every cursor picker of that shape reads as a prompt, and closing a
  role clears what it had open. The fixes have not been through a real run.
- Not verified: restart recovery, stalls and the rate-limit retry on these three runners; a
  real producer other than Claude Code.

## S11: the cursor-agent adapter (2026-11-11)

- Measured first (S11a): cursor-agent 2026.11.05 was probed live in a scratch hc session —
  `--model` argv and the on-screen id, positional prompts on fresh and resumed chats,
  `create-chat`/`--resume`, the workspace-trust dialog and `--trust`, the permission picker's
  `y`/reject keys (a denied command is retried by the runner), the `ctrl+c to stop` and braille
  `Composing`/`Running … tokens` working markers, no OSC 7501 anywhere, and chats under
  `<config>/chats/<md5(cwd)>/<uuid>/` with `meta.json` holding `createdAtMs` and `cwd`. A second
  pass measured `CURSOR_CONFIG_DIR`/`CURSOR_DATA_DIR`/`CURSOR_SANDBOX_POLICY_DIR`: chats land in
  CONFIG, projects in DATA, `~/.config/cursor/auth.json` and the global `cli-config.json` stay
  untouched, while `~/.cursor/mcp.json` is still read and a daily cleanup marker still writes
  globally. The measurement report is `backlot-work/briefs/S11a-cursor-measure.report.md`.
- Done (S11b): `runners/cursor.rs` launches `cursor-agent --trust --model <model> [prompt]` and
  resumes with `--resume <id>` before it — never `--print`, `--force`, `--yolo`,
  `--approve-mcps` or `--workspace`. `Launch` gains `prep`, run by `runners::prepare` before
  every start and resume: the role's `CURSOR_*_DIR` roots under `roles/<role>/cursor/` (dirs
  700, files 600), `cli-config.json` with `permissions.allow` `Shell(...)` rules spliced so every
  other key — `authInfo` included — keeps its bytes, and
  `data/projects/<slug>/mcp-disabled.json` naming the global mcp.json's servers so the role sees
  none of them. A nonempty inherited `XDG_CONFIG_HOME` is a plan refusal (`inherited
  XDG_CONFIG_HOME`); prep failure is `cursor prep failed` attention, never a half-configured
  start. Session discovery reads `meta.json` under the role's `config/chats/` for the newest
  eligible uuid matching cwd and `createdAtMs >= started_ms`. Blocked is the trust dialog or the
  three permission markers (`Run this command?` + `Not in allowlist:` + `→ Run (once) (y)`);
  working is `ctrl+c to stop` or the braille spinner line — a transcript `Running the …` line is
  neither. Approvals map allow → `y` and deny → nothing (the runner retries a rejected command);
  the prompt block runs from the full-width `─` rule above the ask line, and a cursor trust
  dialog is the user's even for an actor. `tools/cursor-stub.sh` is the test-only stand-in:
  `--version`, a cursor-shaped idle screen, its env echoed, and a deterministic chat directory
  with `meta.json`.
- Measured: unit tests pin the slug, the `Shell(...)` rules per role, fresh/resume argv, the env
  roots, the XDG refusal, the prep tree and modes, the mcp-disabled list, the splice preserving
  foreign config keys, and session discovery against fake metadata trees (including malformed and
  missing `meta.json`); work and permit tests cover the trust dialog, the permission prompt, both
  spinner forms, the transcript false positive and the prompt block. The ignored
  `real_stage_cursor_director` runs a real scratch core with the stub first on `PATH` and an
  emptied `XDG_CONFIG_HOME`: the director's terminal comes up on the stub's screen, the isolated
  env lands, discovery records the stub's uuid as `role.session`, and `status --json` exposes it.
  Gates: `cargo fmt --check`, `cargo clippy --all-targets -- -D warnings`, `cargo test` and the
  serial ignored `real_stage` run all green on every commit.
- Not verified: the real `cursor-agent` has never run as a studio role — the stub proves the
  plumbing, not the binary. Whether its `Shell(...)` rules actually pre-allow, the trust marker
  lands under `CURSOR_DATA_DIR`, and the chat layout matches on a live launch waits for the
  controlled director run (S11c). A hook path like Claude's does not exist — cursor has no
  settings-file hooks the stage can wire, so its roles read from the screen only.
- Run (S11c): one controlled run — cursor-agent 2026.11.05 as director, every other role `fake`
  — closed the full flow (dispatch → cast → delivery → checks → accepted review → merge →
  report → `studio.finished`) in 48 events. Isolation, MCP hiding (`mcp list` showed the server
  `disabled`), the trust marker, prompt-free `backlot`/`git merge`/check runs, same-uuid resume
  after a kill, authInfo leak scan and the global diff all passed; the live-director proxy check
  was not measured (the run closed first). Five read-only prompts — `git diff`, `git log`,
  `git branch`, `rg`/`strings` — exposed the director's missing read-only rules, and
  `runner_version` recorded the IDE's version via `cursor --version` while a `cli-config.json`
  landed 664 under the user's umask 002. Report:
  `backlot-work/briefs/S11c-cursor-real-run.report.md`.
- Done (S11d), the run's fixes: the director's read-only git set (`git status`, `git diff`,
  `git log`, `git show`, `git merge` — never `git branch`) is pre-allowed on both cursor's
  `Shell(...)` rules and claude's `Bash(...)` tools; the director prompt names the merge
  command `git merge backlot/<studio>/<task> -m <message>`; `runner_version` asks the launch
  program's `argv[0]` (`cursor-agent --version`), cached per program; `cursor` and `devin`
  record no `effort` — the depth is part of the model id — and `studio new` refuses a set
  `effort` on cursor; and `roles/<role>/cursor/` is held at 700 (prepare forces it and
  `config`/`data`/`sandbox`, discovery re-checks the root), an open dir healing with one
  informational `cursor dir was open` attention.

## S9g: an actor's terminal carries its own key (2026-11-11)

- Done: a real run's `backlot deliver` from a live actor terminal was refused — `role_env`
  set `BACKLOT_ROLE` to the kind name `actor` while `may_deliver` wants the cast key. The
  terminal's `BACKLOT_ROLE` is now `start.role`: `actor-<task>-c<n>` for an actor on start,
  recast, resume and reopen, the fixed name otherwise. And the other half of the hole is
  closed: a `by.role` of exactly `actor` is refused for every kind at append — it matched no
  actor-key whitelist and fell through to permissive — while replay still reads the v0.1.0
  ledgers that hold its events. The actor prompt now lists each declared `[[checks]]` entry
  by `name` and `run` and says `--check <name>=pass|fail` names one of them (or that the
  cast declares none), and the fake runner's `sh:<command>` input runs a line through
  `sh -c` in the role's own environment and cwd, echoing `fake-sh: exit <code>`.
- Measured: `role_env` unit tests pin the actor key on start, recast and resume; model tests
  refuse a bare `actor` on every kind and still replay an old `actor` log; CLI tests pin the
  refusal text on deliver, review and block; prompt tests cover the declared and
  none-declared casts. Real-core fake-runner: `real_stage_actor_terminal_delivers_under_its_key`
  sends `backlot deliver` and `backlot review` through `sh:` — no `--as`, no test-set
  `BACKLOT_ROLE` — and `task.delivered` lands under `actor-t1-c1` while the review is
  refused with no `task.reviewed`. Gates: `cargo fmt --check`, stable and `+1.99.0`
  clippy, `cargo test`, and the serial ignored `real_stage` run all green.
- Not verified: no real agent has run with the keyed `BACKLOT_ROLE` yet — the fake runner
  proves the environment, not a real runner's behaviour under it.

## S9d: notices that are still true, merged and in order (2026-11-10)

- Done: a queued message carrying a `source_seq` is checked against its source at send time;
  when the source no longer stands — the table lives in `stale::stale_reason` and `design.md` —
  the stage records `message.dropped { target, reason }` and sends nothing. A dropped message
  leaves the outbox and the nudge pass's idle count, still counts as handled in the notices'
  dedup set so it is never re-queued, and is never marked `message.delivered`; reports and
  `source_seq`-less messages are never dropped, and a send already in flight is exempt. When the
  gate opens on two or more pending items the stage writes `messages/digest-<top seq>.md` — one
  `## <label> (seq <n>)` section each, a stage notice's body inlined, a role file or report
  named by path, a file-less message shown as its line — sends the single
  `stage → <role>: read <path> (<n> items)` line, and records `message.delivered` for every
  item. `digest::rank` holds the priority order in one place (documented in `design.md`), ties
  by seq; `backlot status` counts dropped messages apart. The S8 failure — the director's
  notice arriving after its prompt was already answered — now lands as `message.dropped`
  instead of a wasted turn.
- Measured: one unit test per staleness row plus the never-stale rows, the digest's body per
  item kind, the full fourteen-rank order queued so seq alone would invert it, and `queued_for`
  / the outbox / `notices::owed` all leaving a dropped message alone. Real-core fake-runner:
  `real_stage_stale_notice_drops` replays the S8 case — `message.dropped` with `attention
  cleared`, an empty director inbox — and `real_stage_digest_sends_once` queues a `tell` before
  two `delivered` notices and gets one digest line with the notices first and all three
  delivered. Gates: `cargo fmt --check`, `cargo clippy --all-targets -- -D warnings` and
  `cargo test` on every commit; clippy on the CI-pinned 1.99.0 and the serial ignored
  `real_stage` run both green.
- Not verified: drops and digests have only run on the fake runner — no real agent has read a
  digest or had a stale notice dropped — and a notice that goes stale while `terminal.send` is
  still in flight still lands.

## S9c: approvals land on the prompt they were made for (2026-11-10)

- Done: `permit::prompt_block` gives each runner a whitespace-normalised region that names the
  prompt on screen — claude's from the last full-width `─` rule above `Do you want to proceed?`
  with spinner lines dropped, fake's from the last `fake-work: blocked` (with a `prompt:<text>`
  input echoing `fake-prompt:` so two prompts differ), codex's and devin's best-effort from their
  own markers. Before a recorded `role.approval`'s key goes out the live screen's block is
  compared with the saved `attention/<seq>.txt`'s: a mismatch sends nothing, clears the stale
  attention `prompt changed before the decision was sent` and raises a fresh one on the screen on
  show — `needs_user` computed on it — and a screen with no comparable block refuses the same
  way, while a runner with no approval keys is unchanged. Separately, a role out of its blocked
  episode for a whole window with no decision pending clears `prompt answered elsewhere`, so a
  prompt answered in the terminal itself can no longer hold its attention over the next prompt.
  The stage now tracks an `unblocked_since` clock beside `blocked_since`.
- Done: claude launches pre-allow per role, argv only — actors `git add`, `git commit`,
  `git status`, `git diff`, `git log`, `git show` (each bare and starred) plus every declared
  check's `run` verbatim; the director `git merge` plus the checks; producer and writer keep the
  base pair. A `run` with a parenthesis or comma is not expressible and never becomes a rule, and
  `studio new` refuses a `run` reaching for `BACKLOT_*`, `env -i` or `--ignore-environment` — the
  same words `touches_env` flags — so no pre-allow ever covers one.
- Measured: three duplex-core tests — the answered-on-terminal race (no key to the new prompt,
  `prompt changed` clear, fresh attention with its own saved screen and its own approval landing),
  the `needs_user` recompute on a `BACKLOT_`-reaching replacement prompt, and the
  answered-elsewhere clear plus next-prompt raise; `prompt_block` unit tests cover the spinner
  timer, a changed command, whitespace moves and every runner's markers; argv tests pin the exact
  `--allowedTools` value per role with and without checks and skip the parenthesised `run`;
  `studio new` refuses all three unsafe `run` shapes and accepts a parenthesised one. On a real
  core: the same race and flag recompute (`real_stage_approve_lands_on_the_prompt_it_was_made_for`,
  `real_stage_approve_recomputes_needs_user`) and `real_stage_prompt_answered_elsewhere_clears`
  all pass on the fake runner.
- Not verified: whether Claude Code matches the new `Bash` rules as it did the `backlot` one, and
  whether a wrapped `X=y git commit …` slips past them — recorded in `limits.md` for the S9f
  real-session run. The codex and devin prompt blocks are unmeasured.

## S10: merge hc-tui main at 4e8731c — v0.1.2 (2026-11-10)

- Done: upstream's nine commits landed as merge `f0691c2` — one synchronized write per frame,
  copied image files and clipboard-miss notices for remote image paste, popup selection indexed
  by the frame stride, popup pastes encoded under the popup terminal's own bracketed-paste mode,
  the paste-matrix and real-agent smoke e2e helpers, and the version bump. Four conflicts all
  resolved for backlot: the workspace, `hc-vt` and `Cargo.lock` keep version 0.1.0, and the paste
  arm keeps `panel_focused()` eating paste ahead of upstream's new `popup_paste`. The new e2e
  helpers run `CARGO_BIN_EXE_backlot`, and the merged user docs say `backlot` where they name the
  installed binary; `NOTICE` and the `Derived from herdr` headers are untouched.
- Measured: one new test pins the merge's routing — a focused panel eats a host paste with a
  popup open, and releasing panel focus lets the same paste reach the popup as a semantic Paste.
  Gates: `cargo fmt --check`, `cargo clippy --all-targets -- -D warnings`, `cargo test` (every
  suite, 0 failures), `sh scripts/test-install.sh` (29 checks), and against the pinned `hc` the
  ignored `hc-proto` integration (5), `real_core_smoke` (1), `real_stage` (6) and both
  paste-matrix e2e runs (2) all pass on owned scratch cores.
- Not verified: `real_agent_smoke_e2e` stayed ignored — it launches the real agent CLIs, so it
  was compiled but never run. The parity harness was not run; the maintainer runs it.

## S9a: who owes the next step (2026-11-10)

- Done: `backlot_model::steps::next_steps` derives every obligation the ledger still holds — the
  plan pair, dispatch, task split, cast, delivery, review, unblock, the node report, the
  producer's report to the user and finish — each keyed on the seq of the event that created it,
  with an owner and a since-when. The stage's nudge pass tracks an in-memory idle stretch per
  role (live, terminal up, `waiting`, nothing queued, no send in flight) and queues exactly one
  `purpose: nudge` message per step after `nudge_after_s`, then raises one `owed` attention when
  it stays owed and idle `escalate_after_s` past delivery; a non-producer owner also notices the
  producer, and the attention clears when the role owes nothing or is no longer wanted. Both
  delays are cast-file keys, serde-defaulted on `studio.created`, with `0` refused like
  `stall_limit_s = 0`. Nudges share `source_seq` with ordinary notices without suppressing
  either, and the writer's wanted clock follows `plan`/`dispatch` purposes alone. `backlot
  status` prints one `next:` line per owed step, the summary document carries the first as a
  capped optional `next`, and the panel draws it under the studio like the `error` line.
- Measured: 15 model tests cover every obligation row and none after `studio.finished`; 6 stage
  tests cover the producer and director nudge paths, working/busy owners, restart replay, the
  shared `source_seq` and the user's step never being pushed; cli tests cover both `0` refusals
  and set values reaching the ledger; the summary and panel tests cover the `next` field and the
  drawn line; the ignored real-core test ran green on the fake runner — plan published, the
  producer's inbox got the single nudge, dispatch removed the `next:` line (7.7 s). `cargo fmt
  --check`, `cargo clippy --all-targets -- -D warnings` and `cargo test` pass on every commit.
- Not verified: no real agent has been nudged or escalated yet — the timings are untested against
  real idle stretches, and the escalation notice to the producer has only run in unit tests.

## S8: restart recovery on real agents, first release (2026-11-10)

- Run, approved by the user: Claude Code 2.1.321, `claude-haiku-5-5` at low effort, a scratch core
  on the followcat fork, at most five sessions (producer, director, writer, two actors — a sixth
  was never started) and 30 minutes. The hc core was stopped and started again twice: at 206 s,
  with both actors working and one at a permission prompt, and at 442 s, with the director at a
  prompt to merge t2. Both times nothing of the core was left after the stop. After `stage
  start`, every live role logged `role.lost` (`terminal gone`) and `role.resumed` (`recovered`)
  with the session id it had, inside a second; the transcript files of those ids kept growing;
  closed roles stayed closed; the prompt open at the stop cleared with `role lost`. Both actors
  delivered, and the director accepted t1 and t2 and merged both into integration (13 tests OK).
  It retried the merge the second stop interrupted, after checking it had not landed. The
  studio finished after 17 min 18 s. Of 21 permission prompts the director answered 12 and the
  person 7, one of them denied.
- Found and fixed, in the prompts: the director's "report at the nodes" named no node, so after
  both merges it asked for one on its own screen and waited about four minutes. It now reports at
  the `integration` node and sends questions through `backlot tell producer`. After the recovered
  notice the producer reviewed t1 itself; it now leaves reviews to the director after a dispatch.
  `prompts_tests.rs` pins both. The new wording has not run on real agents.
- Seen, not changed: attention notices queued for a busy director arrived after their task had
  been accepted (limits.md, "A notice can arrive late or stale"); the director recognised them as
  stale.
- Release: the workspace version goes back to 0.1.0 — 0.1.1 came in with the hc-tui merge — and
  the README no longer says the first release is pending.

## S7: user docs, known limits, a clean-machine install check (2026-11-10)

- Done: `docs/studio/guide.md` walks one studio start to finish — what you need, the cast
  file (one complete example and every key), starting, talking to roles, the task flow, permission
  prompts, a midway requirement revision, every attention reason with its way on, restart
  recovery, finishing and cleanup, the state layout and the panel's config. `docs/studio/limits.md`
  says plainly what real agents ran (three recorded runs on Claude Code; launch and session-id
  capture only for the rest), what is fork-only, and the detection, timing and cleanup limits.
  The README describes the studio, gains a "Run a studio" section and links both docs; `backlot
  --help` points at the guide and `docs/keys.md` lists `prefix+y` and the panel's keys. The
  attention reasons now live in `src/reasons.rs` — `launch refused`, `not a git repository` and
  `work area failed` became constants beside the existing ones — and `docs_tests.rs` checks the
  docs against the code: every fenced `backlot` line in the guide and the README's "Run a studio"
  parses through the real argument parser (neutral values for `<...>`), the guide's cast file
  goes through the `studio new` build, and every reason in `ALL` is in the guide's table.
  `release.yml` gains `workflow_dispatch` and an `install-check` job on ubuntu-latest and
  macos-latest for each native target: the built archive is served from a local mirror in
  install.sh's layout, the real installer runs with its official-core step on, `backlot --version`
  and `hc --version` come from the install dir, and a stub `claude` (answers `--version`, stays
  alive reading input otherwise) stands in while `studio new` and `stage start` run under an
  empty HOME — then the job waits for the producer's and director's terminals and for `backlot
  status` to show them live, prints the stage screen and status whatever happens, and stops the
  core. A platform the official installer has no build for is reported and its studio step
  skipped rather than faked.
- Measured: the four docs tests are green; `cargo fmt --check`, `cargo clippy --all-targets --
  -D warnings` and `cargo test` (1227) pass; `scripts/test-install.sh` still passes (29 checks);
  the workflow parses through `yaml.safe_load`. Corrections the check made to the supplied
  limits list: the record at the time showed two real runs — the third ran the same day and the
  S5b entry below records it; OpenCode gets no approval key at all by design, which is stronger
  than "partly unmeasured"; `git` calls on the stage's loop have no timeout at all (the `--version` and
  session probes do, at 5 s and 10 s); the panel reads only the local core's document, so a
  remote studio shows nothing; and a held role's restart after terminal loss is unmeasured on a
  real core.
- Measured after the merge: the `install-check` job passed on ubuntu-latest and macos-latest. The
  official installer put `hc` 0.1.19 on each runner, and `backlot` built from the release job
  started a studio there with the producer and the director live.
- Not verified: no real agent ran anywhere in that job (the `claude` it starts is a stub); the
  one-line install from a real published release has not run, because there is no release yet.

## S7a: merge hc-tui main at 2c85e84 (2026-11-10)

- Done: upstream's 35 commits landed as merge `d05843b` — keyboard copy mode, PgUp/PgDn
  scrollback, image paste, OSC 52, the input-drain fix, starting a missing local core,
  `install.sh` installing `hc`, and the user-doc rewrite. All ten conflicts kept both sides:
  copy mode routes before the panel (a copy-mode search prompt takes even the panel chord's
  keys), the chord still toggles from normal mode, a focused panel eats text and image paste,
  hide-cursor keeps the panel-fullscreen and Copy arms, and the tick runs `tick_panel` plus
  `tick_selection` and `tick_copy_feedback` independently. Installer and docs keep the backlot
  name, `BACKLOT_*` variables and the releases/download URL while taking the core-install
  behavior (`BACKLOT_SKIP_CORE`, `BACKLOT_CORE_URL`); upstream's `github.io` install shortcut
  and its pages workflow were dropped.
- Measured: four new tests pin the merge decisions — the chord toggles the panel from normal
  mode, a copy-mode search prompt receives the chord's keys, a focused panel eats an image
  paste, and `tick_panel` runs on a tick where the selection also ticks. All gates green:
  `cargo fmt --check`, clippy with `-D warnings`, `cargo test` (every suite, 0 failures),
  `parity_cells` (3), the parity Python tests (31), `test-install.sh` (29 checks covering
  `BACKLOT_SKIP_CORE` and a stub `BACKLOT_CORE_URL`), `test-fetch-zig-deps.sh` (15),
  `real_core_smoke` (1) and `real_stage` (5) against the real pinned `hc`.
- Not verified: the parity harness was not run — the maintainer runs it. No real agent or live
  panel session was exercised, and no release was tagged.

## S6b: recover after a restart (2026-11-10)

- Done: a `backlot stage start` after an hc restart brings the studio back from the ledger alone. A
  wanted role whose terminal is gone is recorded `role.lost`, its open permission or stall
  attention clears in the same pass, and the role starts again resuming its recorded session as
  `role.resumed` with reason `recovered` — deduped by a crash-loop guard of three losses in ten
  minutes kept in `RoleState`, so the count survives replay. A lost role with no session, or past
  the guard, raises one `cannot resume` attention routed to its reviewer like a stall. The resumed
  role gets one `recovered` notice through the usual outbox and send gate. A corrupt ledger or a
  failing act, watch or deliver now skips that one studio — logged once, shown as an `error` line
  in the summary and the panel — while only a lost core still ends the stage. Session discovery
  keeps looking past the 60-second not-found attention until the ten-minute stop, scoped to the
  role's current life, and `studio new` refuses `stall_limit_s = 0`.
- Measured: reconcile covers resume, guard window, unwant and held-reopen cases; notices cover the
  `lost` routing and the single `recovered`; tick tests run a corrupt ledger beside a good studio
  and a failing pass on a duplex fake core. `real_stage_restart` on the fake runner stops the core
  with an open attention and a queued tell mid-flight, restarts it on the same session and asserts
  every wanted role is lost then `recovered` on the same session with one terminal each, the stale
  attention cleared, notices and the tell delivered, a closed role untouched;
  `real_stage_restart_stage_only` kills only `backlot-stage` and asserts nothing is lost or
  relaunched. fmt, clippy and `cargo test` green on every commit; `real_stage` (2) green against
  the pinned core.
- Fix round: a broken ledger is retried every tick — a same-length repair recovers — and a failing
  pass keeps its latest error beside `broken`, shown in the summary until a pass succeeds. One
  helper, `Fail::call`, classifies every call error: only `Closed` and `Protocol` — a dead wire —
  or a lost event stream end the stage, while a timeout, full queue or unoffered method is that
  studio's refused pass and a failed terminal-list refresh keeps the last inventory. A wanted
  lost role whose terminal is already live records `role.resumed` `recovered` on that address —
  no second `terminal.run`, checked before the loss guard. `reconcile.rs` split into `desired.rs`
  and `recover.rs`. Both `real_stage_restart*` runs green three consecutive times against the
  pinned core, logs kept outside the repo.
- Not verified: no real agents ran a recovered session — the resume flag is the fake runner's, and
  whether a real runner picks a stopped core's session back up is untested.

## S6a: stalls, the rework cap, going on after a block (2026-11-10)

- Done: three real rounds found four gaps and all four are closed. A live role whose screen is
  unchanged for the studio's `stall_limit_s` — a new optional cast-file key, default 600 s — raises
  one `stalled` attention per frozen episode with the screen saved under `attention/<seq>.txt`, and
  a changed screen clears it; the rule reads the send gate's `Screen` stability track, so detection
  costs no extra `terminal.read`. An actor stalls only on a `running` task — `working` and `waiting`
  both count — and the producer, director and writer only while `working`; `blocked` never counts.
  The notice goes to the actor's task reviewer, the writer's and director's to the producer, the
  producer's to nobody. The review that takes a task to `at_cap` now owes the producer one `at-cap`
  notice naming the verdict, the rounds and the three ways on, and the actor nothing it could not
  act on — below-cap reviews keep their `rework` notice. `task resume` validates from `blocked` as
  well as `paused` and replays to `running` with the same cast and session — a pause of a blocked
  task still restores `blocked` — and a resume that lands on `running` owes the actor one `resumed`
  notice naming who resumed it (`StateChangeRecord` keeps `by` and the `resumed_to` outcome for it).
  A denied non-actor's notice no longer names the actor-only `backlot block`.
- Measured: the `stall` truth table, stage-level episode tests on a fake client — raise once per
  episode, clear on change, re-raise — notice routing for actor, writer, director and producer, the
  cap below/at/second-pass cases, resume validation and replay including pause-of-blocked and
  pause-of-delivered restores, actor and non-actor deny wording, and the prompt parser covering the
  new `backlot` lines. `real_stage_watch` on the fake runner with `stall_limit_s = 5` raises
  `stalled`, queues the director's notice, clears on a changed screen, then blocks, resumes and
  delivers — run here against the pinned core. fmt, clippy, `cargo test` and `real_stage` (3) green
  on every commit.
- Not verified: no real agents ran the new prompts or the stall notice flow. Rate-limit and
  connection-error screen patterns are not built — nothing measures them — so such a role reaches
  its reviewer as a stall. A stage restart loses the screen track, so an open stall clears and can
  re-raise one limit later. Restart recovery is S6b.

## S5b: third real run (2026-11-10)

- Done: the loop closed again end to end on real Claude Code agents — a requirement through plan,
  tasks, deliveries and reviews to an integration branch, in 12m48s and 83 ledger events across
  five sessions and two casts. Both task branches were merged into the integration branch and
  eight tests passed there; the sample repository's own `main` was untouched.
- Measured: eleven permission prompts over the run, answered by the director and by the person;
  two actors ran at once.
- Not verified: one actor hit a denied command, blocked and had no way on, so its task ended
  taken over with no delivery or review. S6a closed that gap — `backlot task resume` unblocks a
  blocked task — and that fix has not run on real agents.

## S5b: fixes from the second real run (2026-11-09)

- Done: a sent approval key now ends the blocked episode — `blocked_since` is forgotten when the
  clear is recorded, so a prompt already up (or still up) after the answer starts its own count and
  raises its own attention at the full window; in round 2 a back-to-back `git commit` prompt sat
  four minutes unraised. A `deny` approval owes the denied role a notice — any role, not only
  actors — naming who denied it, that the command did not run, and pointing at `backlot block`;
  `ApprovalRecord` keeps `by` for it, and Claude Code's `esc` is measured on 2.1.319 to interrupt
  with `Interrupted · What should Claude do instead?` and wait. The writer's last step now sends
  `backlot tell producer --file <file>` — the director acts only on the producer's dispatch, which
  is what the seven-minute idle waited on. All four prompts' shared rule sends a file that is only
  input to a `backlot` command to the session's scratchpad directory — the system prompt names it —
  never the repo or the work area.
- Measured: a stage-level test on a duplex fake core runs two blocked episodes with no unblocked
  sample between them — the second attention raises at its own window, never sooner, and the spent
  decision sends no second key; `real_stage_closed_loop` chains `work:blocked:2` through the fake
  runner end to end and asserts the `denied` notice lands in the actor's inbox. Notice unit tests
  cover actor and non-actor denies, dedup on `source_seq`, and `allow` owing nothing; prompt tests
  pin the writer's `tell` line and the scratchpad rule in all four. fmt, clippy (stable and
  1.99.0), `cargo test` (1106) and `real_stage` (2) green.
- Not verified: the changes have not run on real agents; Codex's approval keys remain unmeasured.

## S5b: the role prompts after the first real run (2026-11-09)

- Done: all four role prompts carry the same one-command rule — a `backlot` command runs one per
  shell call with no `&&`, `;`, pipes, `cd` or shell variables around it, since anything chained
  falls outside `Bash(backlot *)` and waits on a person; files are written by the file-writing
  tool and passed by absolute path, and read by the file-reading tool. The producer learned where
  the published plan and acceptance list live, that accepted work sits on the integration branch
  rather than the user's checkout, and that an actor's permission prompt is the director's —
  never relayed to the user, never a mode change. The director learned `task resume` before
  casting a paused task, shared-branch inspection from its own area (`git show`/`git diff`, no
  `-C`, no `cd`), and one-command merging; the actor learned one-command `git` and tool-based
  reading. A new test pins the shared rule in all four and bars `&&`, `auto mode` and
  `skip-permissions` outside it.
- Measured (first real run: 5 Haiku-class Claude Code roles, one full loop, 2.1.319):
  `--allowedTools` works, Claude's `allow` key is `enter`, a chained command does not match
  `Bash(backlot *)`, and the closed loop ran end to end on real agents. The run needed 33
  permission prompts — 20 from the producer, director and writer — which this tuning addresses.
- Not verified: the tuned prompts have not run on real agents; Codex's approval keys remain
  unmeasured.

## S5b prep: pre-allowed pair and pinned identities (2026-11-09)

- Done: `LaunchCtx` carries the studio's resolved state directory; the Claude Code adapter adds one
  `--allowedTools` pair — `Bash(backlot *)` and `Read(//<studio dir>/**)` — before `--model` on fresh
  launches and resumes, so a role can run `backlot` and read its prompt and message files without a
  permission prompt. Nothing broader is allowed and the permission modes are unchanged; other
  adapters ignore the field. A pinned role terminal can no longer claim another identity:
  `--as`/`--studio`/`--task` flags disagreeing with `BACKLOT_ROLE`/`BACKLOT_STUDIO`/`BACKLOT_TASK`
  are refused before the command runs, `studio new` included.
- Measured: argv tests assert the exact pair on fresh launches and resumes, a `--flag` after the
  variadic value, the prompt last, and the absent bypass rules on both profiles; CLI tests cover the
  pin refusals (both values named, nothing appended), equal flags and the unpinned case. fmt, clippy
  (stable and 1.99.0), `cargo test` (1102) and `real_stage` (2) green.
- Not verified: the `--allowedTools` rule syntax is from Claude Code's permissions documentation —
  still to be measured on 2.1.319 in S5b — as is an env-prefixed or wrapped `backlot` command not
  matching `Bash(backlot *)`.

## S5a: The closed loop's mechanics (2026-11-09)

- Done: the stage closes the loop end to end on the fake runner. Role launches ensure git work
  areas first — `work/integration` on `backlot/<studio>/integration` for the director,
  `work/<task>` on `backlot/<studio>/<task>` for each actor, planned over injected git and reused
  on recast and reopen; a non-git repo raises `not a git repository` and starts neither, and a git
  failure raises `work area failed` once with the first stderr line. Producer and writer stay in
  the studio repo. Fresh starts render a role prompt from `prompts/<role>.md` into
  `prompts/<role key>.md` and hand the runner one line (`Read <path> and follow it.`; Devin gets
  the file form); resumes get none. Queued messages and reports deliver oldest-first per role as
  `[backlot] <from> → <to>: read <file>` behind a live/waiting/stable-screen/uptime/send-gap gate,
  one `terminal.send` per role at a time spawned off the loop; `to: user` is never sent.
  Stage-generated notices (delivery → caster, rework review → actor, blocked task and
  requirement revision above v1 → director, actor permission prompt → director) queue as ordinary
  messages deduped by `source_seq`. A role sampled blocked for two seconds raises one
  `permission prompt` attention per episode with the screen saved under `attention/`; `backlot
  approve` records `role.approval` (refused without an open prompt) and the stage sends the
  runner's key while the prompt still shows — never without a decision at or after the attention —
  then clears; a gone prompt clears with no key. `studio finish` refuses while tasks are open,
  then leaves `desired` holding only reopened people. The summary marks a behind actor's `doing`
  as `task t1 · behind vN`; the panel draws it verbatim.
- Measured: the model suite (65) covers the new events and fields; stage tests (122) cover
  work-area planning over a fake git runner and one real repo, prompt render and every prompt
  command through the real parser, each delivery gate on its own, oldest-first, a refused send,
  owed notices once per source event across restarts, approval keys per runner, episode dedup,
  and finish's reopen-only desired. `real_stage_closed_loop` drives the full loop on a scratch
  core — requirement, plan request (writer starts and is told), dispatch (director is told),
  cast into its own worktree and branch, delivery and rework notices, a midway revision notice,
  two blocked episodes answered by `backlot approve` (enter, then esc), a finish refusal naming
  the open task, acceptance, finish, and a reopen after finish. `real_stage_loop` still passes.
- Key findings: the light path needed no change — `task new`, `cast` and `review` have no caller
  restriction, and `cast.by` routes the delivery notice to the producer. The run exposed two
  bugs: `approve` was missing from the binary's command list, and an approval must be at or after
  the open attention or a stale decision answers a new prompt.
- Not verified: prompts are untested on real agents (S5b tunes them), Claude/Codex approval keys
  are convention not measurement (runners.md), the panel's behind-marker was drawn only in the
  row test, and worktree removal is out of scope — trees accumulate under `work/` until a later
  step.
- Rework 1 (2026-11-09): six fixes from reading the code against real runners. The folder-trust
  dialog no longer counts toward a permission episode — it neither raises nor holds one, and it
  restarts the two-second clock for a prompt right after it. A `terminal.send` that answers
  `sent: false` stays queued with its reason logged (a successful call's `SendResult` is the
  delivery — this core has no `sent` field). Send collection drains only the calling studio's
  finished sends, so `message.delivered` can no longer land in another studio's ledger. Task
  notices follow `cast.by` only for `producer` — a `user` cast now reaches the director instead
  of an undeliverable `to: user`. Producer and director prompts name `<studio dir>/req/` as the
  requirement's place instead of rendering an empty file at v0 (`backlot status` already shows
  the version). Devin's approval keys are mapped — `enter`/`esc`, observed on 3000.12.32.
- Rework 2 (2026-11-09): paths through a symlink, found by main's red macOS CI. `worktree_listed`
  now resolves the looked-for dir before comparing — git prints resolved paths, so a state dir
  under a symlinked parent no longer retries `worktree add` on an existing worktree — and the
  stage resolves its state base once at startup, so role dirs, prompts and `BACKLOT_STATE_DIR`
  all compare equal to the OS-resolved `term.cwd` the trust check sees. `err_line` prefers the
  `fatal:`/`error:` line over progress noise like `Preparing worktree`.

## S4b: The studio panel (2026-11-09)

- Done: the client reads the local `backlot` document through the watcher's new
  `local_documents` param (this core only; remote machines never carry the stage's document)
  and parses it into `backlot_model::summary::Summary` — `backlot/1` only, a missing, malformed
  or foreign-schema document reads as `no studios`, and an unknown `work` degrades to waiting.
  A right-column panel draws it: `Closed` (default, zero width, no keys, no timer),
  `Collapsed` (16 columns, working groups or `idle`), `Expanded` (configured width 24–60,
  default 30). Beside the panes only past the mobile threshold with 60+ columns left for them;
  otherwise it covers the frame and the pane geometry keeps the closed shape. All five
  `layout_config().layout(...)` call sites go through `chrome_layout`. Expanded shows studios,
  the four role groups (each collapsible, empty groups skipped) and people — mark, name,
  priority flag, runner/model, `doing`, attention — with five distinct mark+colour pairs from
  `chrome/status.rs`; working marks spin on a ~120 ms one-shot deadline armed only while a
  working mark is drawn. Selection, clicks and Enter act on stable targets, never indices.
  `prefix+y` toggles; focused keys move/collapse/flip/act; `+`/`-` run `backlot priority` and
  a two-step Enter runs `backlot reopen` on resumable closed people — off the event loop via an
  injectable runner (`current_exe()` in production), failures notice with the first stderr
  line. Mouse: title flips, rows act like Enter, wheel scrolls, the scrollbar drags, an outside
  click unfocuses. Without a live `backlot-stage` terminal the panel shows `stage not running`
  and keeps the last document. Panel settings live in `$XDG_CONFIG_HOME/backlot/config.toml`
  (`[panel] width`, `key`) — malformed files notice with line:column only; herdr's config is
  untouched.
- Measured: 45 new tests — the row model (grouping, order, collapse, titles, idle), document
  reading (missing/malformed/wrong schema/unknown `work`/stage liveness), config (defaults,
  clamp, malformed position, path), the renderer (distinct marks and colours, the animated
  working mark, overflow scrollbar, stage line, selection), and App wiring (geometry at
  200/120/80/40 beside and full-screen, the toggle cycle, every focused key, two-step reopen,
  injected priority argv, the failure notice, title/row/outside clicks, wheel and scrollbar
  drag, the deadline arming only while working, and a closed panel identical in geometry,
  forwarded keys and cells). `cargo fmt --check`, `cargo clippy --all-targets -- -D warnings`
  and `cargo test` (1039 tests) green; clippy also green on the CI-pinned 1.99.0 toolchain;
  `real_stage` passes on the real core.
- Key finding: `y` is unbound in the reference default keybinds and nothing in
  `scripts/parity/` sends it, so `prefix+y` is safe; a configured herdr binding on the same
  tail wins while the panel is closed or unfocused.
- Not verified: the full parity harness was not run (the reviewer runs it against a saved
  baseline). The panel landed inside `hc-tui` (`src/panel/` + `src/app/panel.rs`), not a
  separate `backlot-panel` crate — design updated. The stage-down state shows the line only;
  starting the stage from the panel is not implemented.
- Deviations from the design brief: none beyond the crate placement and the stage-down offer
  above.
- Rework 1 (2026-11-09): real-TUI fixes — a full-screen panel resets its rect before painting
  and owns the keyboard (`Esc`/Enter/one chord all close it; beside semantics unchanged), a
  beside reservation can no longer flip the chrome to the mobile layout, the spin deadline is
  armed only when none is pending, runner text advances by display width past wide-character
  names, an unparseable `panel.key` falls back to `y` with a file-naming notice, `Esc` or
  activating another row disarms a pending reopen, and the wheel bounds by the scroll
  metrics' line maximum.

## S4a: The data behind the panel (2026-11-09)

- Done: tasks carry a `priority` (`low`/`normal`/`high`, default `normal`) — `task.cast` gains an
  optional `priority` (absent keeps the current one, old logs load) and `task.priority` changes it
  on an existing task; it orders the panel's display only and nothing in reconcile reads it. A
  closed person can be held: `role.reopen`/`role.release` set and clear `RoleState.held`, and the
  stage wants every held role back — desired with `Start.resume = <its session>`, merged into one
  entry when the role is already desired — so `act` resumes it and records `role.resumed` (reason
  `reopened`, original session ID) rather than `role.started`. New commands `backlot cast
  --priority`, `backlot priority`, `backlot reopen` and `backlot close --person`, all gated to the
  user and the director. `runners::work_state` classifies a visible screen per runner — each
  adapter's blocked prompt before its mid-turn marker, else `waiting` — and the watch samples one
  `terminal.read` (`lines: 0`) at most once a second per live role into a `Stage` `(studio, role)`
  map, keeping the last value on a refused read and dropping the entry when the role stops being
  live; it never reaches the ledger. The summary (schema still `backlot/1`) gained `work`, `task`,
  `priority` and `resumable` on roles — session IDs stay out — and actors sort open-first, then by
  priority high-first, then role key. `backlot status --json` shows `priority` and `held`. The fake
  runner answers `work:<state>` lines for the whole path.
- Measured: 58 model + 91 stage tests cover priority reduction/validation/old logs/commands,
  every reopen refusal, held roles in `desired` (resume set, no duplicate, actorless skipped),
  `role.resumed` on resume (including post-launch runners), release into `Stop`, the `work_state`
  table per runner with blocked beating working, the summary's new fields and ordering with no
  session IDs, and the one-second sample throttle. `real_stage.rs` passes on a scratch core: an
  actor's `work` walks waiting → working → blocked on `work:` lines, `task abandon` closes it as
  `resumable`, `backlot reopen` resumes it in the same session ID under `role.resumed`, and
  `backlot close` closes it again. `cargo fmt --check`, `cargo clippy --all-targets -- -D warnings`
  and `cargo test` green on every commit; clippy also green on the CI-pinned 1.99.0 toolchain.
- Not verified: OpenCode's blocked and working markers are guessed, not measured (marked in
  `runners.md`); every other runner's markers come from captured screens but have not been watched
  live. Reopen/resume ran only on the fake runner — real adapters' resume-on-reopen is S6
  territory alongside lost roles. A held role's terminal loss restarts it through the same
  desired entry, unmeasured on a real core.

## S3: Runner adapters (2026-11-09)

- Done: `runners.rs` is now a `runners/` module, one adapter file per runner plus `fake`. `plan`
  takes the identity, terminal name, working directory, an optional first prompt and an optional
  session ID to resume, and returns argv, env and how the session ID is known (`Chosen` now, or
  `PostLaunch`). Claude Code, Codex, Devin and OpenCode launch and resume with the measured
  commands in `docs/studio/runners.md`; `standard` and `worktree` permission profiles map to the
  right flags per runner, and an unknown runner or profile is an `attention.raised`, never a
  launch. Claude Code argv is prefixed `env -u` for each inherited `CLAUDECODE`, `CLAUDE_EFFORT`,
  `CLAUDE_PID`, `AI_AGENT` or `CLAUDE_CODE_*` marker; OpenCode gets its model through
  `OPENCODE_CONFIG_CONTENT` and no effort flag. `role.session { role, session_id }` joined the
  model: codex IDs are found in `~/.codex/sessions/**/rollout-*.jsonl` (newest day first, matching
  cwd, created after the role started, unclaimed), devin's via `devin list --format json` in the
  role's cwd, both through an injected exec; claude and opencode IDs read back off the terminal's
  argv for adopted roles. A live role still without a session after 60s raises one
  `attention.raised` ("session id not found"), ever. At most one real runner launches per tick
  (`fake` exempt); `runner_version` comes from `<runner> --version` once per runner per run.
  `[runner_env.<runner>]` in `<state base>/config.toml` reaches the terminal env only — malformed
  config refuses the role. Claude Code's folder-trust dialog is answered with Down then Enter
  once per role when the screen shows it inside the role's own cwd in its first 60s.
- Measured: 45 model + 57 stage tests cover the exact argv per runner/profile/prompt/resume,
  refusals, marker scrubbing, codex/devin capture edge cases (claimed, pre-start, wrong cwd,
  failures), the 60s attention once, `runner_env` parsing and confidentiality, and the trust
  parser. `real_stage.rs` passed on a scratch core with `model = "trust"` making the fake runner
  show the dialog — the stage answered, the role announced itself, the ledger stayed unchanged.
  `cargo fmt --check`, `cargo clippy --all-targets -- -D warnings` and `cargo test` green on every
  commit; clippy also green on the CI-pinned 1.99.0 toolchain.
- Not verified: no real `claude`/`codex`/`devin`/`opencode` launch was run — forbidden by the
  milestone; every argv is unit-tested, not observed. Resume argv is built but nothing calls it
  for a lost or closed role yet (S6). Codex/devin session discovery ran only against fake trees
  and injected output; `devin list`'s exact field names are as specified, not re-measured. The
  trust answer is verified through the fake's trust mode, not a real Claude Code screen. Running
  on official hc remains unverified.
- Rework 1: nine fixes, all found reviewing the first real Claude Code launch or auditing the
  new code. The trust answer now sends one key per tick chosen from the cursor's screen line —
  the two-key burst raced the real dialog — with the once-only latch gone (the 60s window still
  bounds it); the fake's trust mode models the redraw for real. Post-launch runners defer a
  start while a live role in the same `(runner, folder)` pair still lacks its session id,
  across studios, and codex takes the oldest matching rollout so a person's later file cannot
  win a role's claim. Discovery is throttled to once per 5s per role, stops after ten minutes,
  and codex opens only `YYYY/MM/DD` day directories from the day before the role's start.
  `real_exec` drains probe stdout on a thread so a chatty `--version` cannot deadlock. A
  malformed `config.toml` records only line and column — never the secret-bearing line. Claude's
  `env -u` list is the exact set of inherited markers, so `CLAUDE_CODE_OAUTH_TOKEN` and friends
  survive. A configured `[runner_env] PATH` now leads after the exe's own directory instead of
  being overwritten. A `terminal.run` refusal raises `launch refused` attention (deduped per
  role and reason, retry kept). `studio new` refuses a relative `--repo` (exit 2) and
  canonicalizes an existing one.
- Rework 2: the real launch in a git folder showed the trust answer needs pacing, and review
  found two more defects. Keys now wait for the dialog's measured unsafe start — first key only
  after the screen has held two seconds, then one per 1.5 seconds — because a key at first
  paint is lost and an early Enter ended the real runner; the fake models that window. Probe
  output is read into a shared buffer with a bounded post-exit grace, so a grandchild holding
  the pipe (a runner's background service) can no longer stop the stage loop. A refused
  `terminal.run` hands the tick's real-launch slot back, so a role whose spawn always fails
  cannot starve the roles behind it. Session discovery also ends the moment the 60 second
  attention is raised — a role can never claim a file from one that starts after it — which
  makes the ten-minute stop only a backstop; a runner that first writes its session later than
  that stays without an ID.

## S2: The stage process (2026-11-09)

- Done: `backlot stage start` launches a `backlot-stage` terminal whose child runs
  `backlot stage run` — a reconcile loop that watches ledgers and hc events (subscription with
  polling fallback) and exits non-zero on connection loss. `crates/backlot-stage/src/reconcile.rs`
  is a pure function from ledger state + live terminals to ordered actions (stops before starts so
  a recast frees the name); `act.rs` executes them through the public `terminal.run`/`kill`/`rm`,
  recording `role.started`/`role.lost`/`role.closed` around each. Desired roles: producer and
  director always, the writer from a `plan` message until `dispatch`, and `actor-<task>-c<n>` per
  open task's current cast under terminal name `bl-<studio>-<task>`. Stray `bl-<studio>-*`
  terminals are removed, orphan live terminals are adopted. `runners.rs` maps cast identities to
  launch plans — only the `fake` runner exists (`tools/fake-runner.sh`); anything else raises one
  `attention.raised`. `backlot-model` gained `role.closed`, keeping actor and session ID after a
  deliberate stop, and `summary.rs` — the pure, versioned `backlot` document schema — which
  `doc.rs` publishes with change-sensitive CAS writes at most once every two seconds. Default
  state dirs key on `hello().core.id` (`--session`/`--remote` resolve the core once;
  `BACKLOT_STATE_DIR` still wins and needs no core). hc core is untouched.
- Measured: 41 model + 33 stage tests cover reconciliation rules (loss, deliberate stop, recast,
  adoption, strays), `role.closed` semantics, summary shape/caps/drop-off, write throttling on an
  injected clock, state-dir selection and CLI refusals. `cargo fmt --check`,
  `cargo clippy --all-targets -- -D warnings` and `cargo test` green on every commit.
  `real_stage.rs` exercises the whole loop on a scratch core: idempotent `stage start`, labelled
  role terminals, writer across request/dispatch, actor close with no `role.lost`, session ID
  surviving the close, and the document's throttled writes.
- Rework (2026-11-09): three defect fixes. A closed role now finishes a crashed stop only at its
  own recorded terminal address — a recast no longer kills the new actor under the shared name.
  Terminal ownership comes from the `backlot.studio` label (`reconcile::studio_view`), never the
  `bl-<studio>-` name prefix, so one studio cannot stop another's roles or a user's lookalike
  terminal; an unlabelled terminal is never stopped, adopted or dropped. The summary drops the
  role cap: fixed roles list first, then actors by key, every recorded person stays while the
  studio is live (R19), and only finished studios are dropped for size — a live one is written
  even over the 64 KiB target (the hard limit is 1 MiB).
- Not verified: no stall/restart recovery (S6 — a lost role is never relaunched), no real runners
  (S3), and `stage run`'s polling fallback path is unit-tested only — the scratch core always
  offers `events.subscribe`.

## S1: Ledger and recording commands (2026-11-09)

- Done: `crates/backlot-model` holds the ledger's pure model — serde event types, `apply`/`replay`
  reduction, append-time `validate` (candidate-only casts, numbered deliveries pinned to their
  requirement version, latest-delivery-only reviews, the rework cap and `at_cap`), corrections that
  change the view while the original event stays, and cross-studio `records` with no numeric score
  (D8). `crates/backlot-stage` holds the state directory, the `events.jsonl` append-only store
  serialized by `flock` on `events.lock` with torn-tail truncation, and all studio commands with
  `BACKLOT_*`/`--studio`/`--as`/`--task` caller resolution and 0/1/2 exit codes. `main` dispatches
  studio command words to `backlot_stage::maybe_run` before TUI parsing; help points at them. hc
  core is untouched.
- Rework (2026-11-09): studio names are `[a-z0-9_-]{1,16}` checked before any path is built, and
  only `studio new` may create a studio directory — other commands on a studio with no log are
  refused without touching the disk. `task.finding` records defects on any existing delivery,
  including on closed tasks, without changing status; `later_defects` counts only those and is
  attributed to the delivery's actor, while review-first-seen defects moved to `new_in_rework`.
  `wall_time_ms` follows each actor's cast interval instead of the whole task span. At the cap,
  `review` accepts only `accepted`/`partially-accepted` on the latest delivery (a recast restarts
  the budget; `rework_rounds` keeps the task's total), and `backlot task
  pause|resume|abandon|takeover` record the matching `task.state` changes.
- Measured: 32 model tests cover each validation rule accepted and refused, every "Evaluation rules"
  bullet, replay-equals-apply at every prefix, cap exits and findings attribution across a recast.
  16 stage tests cover torn tails, corrupt lines, state-dir resolution, in-process concurrency, the
  name rule, `finding`, the four task-state commands and an at-cap review end to end; 4 binary tests
  cover the scripted studio loop, 8 processes appending 200 events with unique increasing seqs,
  traversal/uppercase/over-length names exiting 2 with nothing on disk, and unchanged
  `--version`/`--session`/unknown-arg behavior. `status`/`records` output verified on the compiled
  binary. All gates green on every commit.
- Not verified: no real stage loop exists yet — messages and reports are recorded, not delivered.
  Role lifecycle events (`role.*`, `message.delivered`, `attention.*`) are modelled and validated but
  nothing appends them until S2's stage. The hc connection, runner launching and the summary
  document are all still absent by design.

## S0: Identity (2026-11-09)

- Done: the `hc-tui` package builds only the `backlot` binary. Version, help and the parse-error
  hint say `backlot`; release packaging, `scripts/install.sh` (`BACKLOT_*` variables), install docs
  and the README follow the new name, and the README records the hc-tui origin and the planned
  studio layer.
- Measured: `backlot --version` prints `backlot 0.1.0` and `backlot --help` shows `Usage: backlot`.
  Every commit passed `cargo fmt --check`, `cargo clippy --all-targets -- -D warnings` and
  `cargo test` (all suites green); `cargo test -p hc-tui --example parity_cells`, the parity Python
  unit tests and `sh scripts/test-install.sh` (20 checks) pass, as does clippy on the CI-pinned
  1.99.0 toolchain. At review, the real-core PTY smoke test passed on the renamed binary in an
  isolated session.
- Not verified: the native parity harness was not run. This milestone only renames the binary and
  user-visible strings, so the harness is first required at S4, where the panel changes the screen.
  No release was tagged, so the renamed install path is exercised only by the offline test script.
