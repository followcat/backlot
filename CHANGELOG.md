# Changelog

All notable changes to backlot are documented here. The format follows
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/) and the project uses
[Semantic Versioning](https://semver.org/spec/v2.0.0.html).

backlot is a fork of [hc-tui](https://github.com/followcat/hc-tui). This file
records backlot's own releases; for the hc-tui releases merged in (up to
v0.1.20), see [hc-tui's changelog](https://github.com/followcat/hc-tui/blob/main/CHANGELOG.md).

## Unreleased

### Fixed

- **Studio-role `backlot tell user` question guard, `--note`, and `--ask` CLI pass-through.** Running `backlot tell user` as a studio role (`producer`, `director`, `writer`, `actor-<task>-c<n>`) with `?` or `？` outside inline code spans, fenced code blocks, and URL query strings (`?key=`) now refuses with exit code 2 unless `--ask` or `--note` is given, adds `--note` for plain-info `tell user` messages (mutually exclusive with `--ask`), preserves `--ask` on CLI `tell user` calls so `message.queued` records `purpose = "ask"` and `RoleSummary.asks` surfaces the question, and updates `producer.md`, `director.md`, `writer.md`, `hollywood-cli.md`, and `docs/studio/` (`guide.md`, `ledger.md`, `panel.md`, `design.md`).

## 0.4.17 — 2026-11-27

### Added

- **Forward-compatible read-only unknown ledger events (`EventKind::Unknown`, write guard, stage pause, TUI banner).** Ledgers with valid JSON lines at `v <= LEDGER_VERSION` and an unknown `kind` now load cleanly into `EventKind::Unknown { raw_kind }` (preserving `seq`, `ts_ms`, `by`, counting toward `next_seq`, preserving raw lines byte-for-byte on rewrite or `studio migrate`, and ignoring unknown payloads in state reduction); `store::append_locked` refuses every write with `this ledger has records from a newer backlot (<kinds>); upgrade backlot`; `backlot stage` pauses read-only for that studio while continuing sibling studios and projecting `needs_upgrade: true` + `unknown_kinds` in `plugin.backlot.summary`; and the Lot, Call Sheet, and studio frames display `"upgrade backlot"` (`docs/studio/ledger.md`).

### Changed

- **Redesigned studio-mode layout, status vocabulary, pill bucketing, and opaque studio chrome.** Unified Cast and Lot role rows onto a single shared `InlineState` line (`RoleBadge`, aligned status word, age/detail, and CJK-safe truncation) while moving runner/model/effort metadata to the center studio header (`HeaderKind::Role`) and preserving role-group `├──┤` dividers. Deduplicated live studios from the left `machines` tree (retaining non-studio workspaces, `stage down`, `finished`, `Archived`, single `backlot <v> · ● ok` status, and bottom-left clickable `? keys`), compressed non-current ready/calm/finished/down studios to 1 borderless row and idle/standby roles in the current studio to `CalmSummary` while the sidebar is open, and removed `actors 0/4` and unfocused Lot footer hints (`─` hairline + structured 3-pair hints appear only when focused). Replaced the monolithic red lot pill with a bucketed pill (`✦ <S> · !<B> · ?<A> · <C>c · ✓<R> ready · alt+y` — counting `Wait::Upgrade` in the blocking `!B` bucket, red only when blocking waits exist, clickable buckets jumping directly to matching Call Sheet items, and `Alt+Y` landing on the first blocking wait or `Compose`), and unified all bundled RGB themes in `!plain` / `!source = "herdr"` mode onto opaque `elevation.surface` chrome (`docs/studio/panel.md`, `docs/studio/hollywood.md`, `docs/theme.md`, `docs/parity.md`).

## 0.4.16 — 2026-11-27

### Fixed

- **Persistent floats keep their configured size across restarts and config reloads.** Adopted floats look up `hc-tui.command` in `[[keys.command]]` on adoption, show, hide, and host resize (falling back to the stored or default size only when no binding matches), so an `80%` float stays `80%` instead of collapsing to half the pane.

## 0.4.15 — 2026-11-27

### Changed

- **Call Sheet zones, unified tonal surfaces, and single focus ring.** Added titled zone rules (`STUDIOS`, `WAITING ON YOU`, `HOLLYWOOD` with inline fold badge), persistent col-2 severity bars, fixed speaker columns with 2-line CJK wrap and relative age, `raised_field` compose styling with persistent empty placeholder, 1-row external drop shadow, shared two-tone footer hints, and unified `elevation.surface` / single-focus-ring styling across sidebar (`backlot` theme), Lot dock, Cast (`┄ closed (n) ┄` divider), and Call Sheet (`docs/studio/hollywood.md`, `docs/theme.md`, `docs/parity.md`).

## 0.4.14 — 2026-11-26

### Added

- **Make-before-break Hollywood agent switch (`backlot hollywood switch`, unified `HwSwitch` picker, `w` / `[switch w]` / `Switch agent…`).** Launches candidate Hollywood beside the live overseer with a redacted ledger brief and extracted conversation tail, commits config/routing only after readiness proof (keeping `hollywood-previous` readable for 10 minutes and rolling back cleanly on failure or cancel), pauses quota-blocked delivery, and blocks live `hollywood setup` without `--force-offline` (`docs/studio/hollywood.md`, `docs/keys.md`, `docs/studio/requests.md`, `docs/studio/guide.md`).

## 0.4.13 — 2026-11-26

### Changed

- **Layered modal popups with dimmed backdrops, breathing padding, rounded borders, and drop shadows.** Modals, wizards, navigator, peek, floats, Task Sheet, Reader, and menus now separate visually from the background using theme-derived surface elevation and shadows, fix wide CJK characters straddling popup and shadow edges, and add `[ui] popup_backdrop = "dim" | "none"` and `[ui] popup_shadow = true | false` toggles (`docs/theme.md`, `docs/keys.md`, `docs/parity.md`).

## 0.4.12 — 2026-11-26

### Changed

- **Studio switching moves to `Alt+PageUp`/`Alt+PageDown`, and `Alt+Left`/`Alt+Right` go back to the terminal.** Studios are a vertical list, so the studio step now lives on the page keys; the old arrow chords were word movement in shells and agent input boxes. `Alt+Up`/`Alt+Down` still move between a studio's people (`docs/keys.md`, `docs/studio/guide.md`, `docs/studio/panel.md`).
- **The Hollywood band shows one row per studio.** A READY row absorbs that studio's asks, other decisions fold into `+n`, and ASKS rows reply or acknowledge in place (`docs/studio/panel.md`, `docs/studio/ledger.md`, `docs/studio/limits.md`).

### Fixed

- **Stepping to a studio also switches the main pane.** `Alt+Left`/`Alt+Right` now shows the studio's last member (a role or a shell, else its producer, else its first live person or shell) instead of leaving the previous studio's pane up. Stepping from a terminal moves the keys to the landed member; stepping from the lot or the machines tree swaps the pane but leaves that surface in charge, so you can keep stepping or press `Enter`. A studio with nobody live leaves the pane as it is, quietly (`docs/keys.md`, `docs/studio/guide.md`).
- **Person questions no longer depend on recency.** `backlot tell user` stays a report, `backlot tell user --ask` is the explicit question, and `ask.acknowledged` lets the user mark one seen without answering it.
- **A rate-limit stall stays one attention.** Reset-aware episodes escalate in place, self-continuing runners wait out their own reset, and Claude's weekly-limit paid-choice dialog is never answered by the stage.

## 0.4.11 — 2026-11-26

### Added

- **Reopen a finished studio.** `backlot studio reopen --studio <s> --as user` records the reopening (`studio.reopened`), and recasting a role, `studio cast set`, adding tasks and answering permission prompts work again. Only you may reopen — a role or Hollywood is refused — and Hollywood may propose it as a `reopen-studio` card. The guide says when to reopen and when to start a new studio (`docs/studio/guide.md`, `docs/studio/hollywood.md`).
- **Hollywood can withdraw its own cards.** `backlot hollywood withdraw <card> --reason <text>` takes back an open card Hollywood proposed, recording who withdrew it and why (`card.withdrawn`); the client shows it as `withdrawn`. Only Hollywood's own cards, never yours or the stage's, and never one already applied, cancelled or stale (`docs/studio/hollywood.md`).
- **A newer card supersedes the older one it conflicts with.** When a new card targets the same single fact an open card already targets — a role's identity (`recast`), a person's terminal (`restart`), a studio's cast (`cast`), its finished state (`reopen-studio`), a studio name (`studio-new`), or Hollywood's identity (`setup`) — the older card is marked `superseded by c<n>` in the same append, so a batch apply can never run both (`docs/studio/hollywood.md`).
- **`backlot guide [<topic>] [--grep <word>]`.** Backlot's own documentation — every file in `docs/studio/` — is compiled into the binary and printed here: no topic lists them, a topic prints one, and `--grep` narrows either to the matching lines (over every topic when none is named). It needs no studio, ledger or core and never searches the disk. Every role prompt and Hollywood's now say their docs come from `backlot guide` and never from a disk search — the `find /`/glob-for-`guide.md` detours that only stopped at a permission prompt are gone — and the command is pre-allowed for every role on every runner that carries an allow list (`docs/studio/guide.md`, `docs/studio/design.md`, `docs/studio/runners.md`).
- **A read-only allow list for every role, on every runner.** `git status`, `git log`, `git show`, `git diff`, `git branch --list`, `git branch --show-current`, `ls`, `rg`, `wc` and `head` are pre-allowed for the producer, director, writer and actors alike, so a role's forensic reads never ask a person — previously only actors and the director got git reads, and only on `claude`, so a producer's `git log --stat` prompted. `claude` (argv), `cursor` and `cmd` (the role's `cli-config.json` / `.commandcode/settings.json`) and `cline` (its `PreToolUse.sh`) carry the set; `devin` now writes it into its per-work-area `permissions.allow` (unverified under `smart`); `codex` and `opencode` take no list. Only `git branch`'s two listing forms are rules — never the bare family, whose prefix would also reach `git branch -D`. The role prompts now name the set and say one plain command per shell call, since a `cd … && git log`, a pipe or `git -C` does not match (`docs/studio/runners.md`, `docs/studio/guide.md`, `docs/studio/limits.md`).

### Changed

- **The cast shows each role in full.** A role's state text and full model (with effort) wrap over up to four rows instead of being cut to the sidebar width; when the people do not all fit, the focused role keeps its height and the rest fall back to two rows.
- **The lot's person rows drop the runner and the model.** The cast already names them, so the row gives those cells to the name, task and state.
- **Roles read as one-cell badges.** `Ⓟ producer`, `Ⓓ director`, `Ⓦ writer` and `Ⓐ actor` (with its task) replace the role words in the cast, the lot, the Call Sheet and the Hollywood frames and band, each in its own theme colour; the state glyph is unchanged.
- **Group rules inside each studio block.** The producer, director, writer and actor groups are separated by a thin `├──┤` rule; an empty group draws none and the cursor steps over the rules.
- **An actor's prompt that waits on an unavailable director is routed to you.** When the director is rate-limited, stalled or gone, an actor's open `permission prompt` — one only the director could answer — is cleared `routed to the user` after a one-minute grace and re-raised `needs the user`, with the reason and the wait in its detail: *the director is rate-limited until 12:10; waiting 14m*. From then the prompt is the person's — the panel and Hollywood's `prompt` notice show it, and the director's `backlot approve` is refused — so a prompt no longer sits for hours behind a stalled director (`docs/studio/guide.md`, `docs/studio/design.md`).

### Fixed

- **A card is resolved and checked when it is proposed, not when it is applied.** A `--cast`, `--brief` or `cast` card `--file` is made absolute against Hollywood's working directory, and one that is not there is refused at propose. `propose studio-new` dry-runs the apply against the resolved files, so a card that could not apply is refused. The cast and brief are copied into the card's directory with their hashes; apply reads that snapshot, and a source file edited since makes the card `stale` (`the cast file changed after this card was proposed`). `card list` and `card show` print a one-line roles summary for a card that carries a cast (`docs/studio/hollywood.md`).
- **The studio name rule is visible.** `[a-z0-9_-]{1,16}` now appears in `studio new`'s usage and in `propose studio-new`'s help, and the refusal names it.
- **Symlinked working paths and cancelled remote attaches behave consistently across macOS and Linux.** Absolute snapshot inputs normalize through `normalize_resolve` just like relative ones, Cursor session lookup compares canonical `cwd` paths, and cancelling an in-flight `Attach::connect` open aborts immediately instead of waiting out `CLOSE_TIMEOUT`.

## 0.4.10 — 2026-11-26

### Fixed

- **The update hint survives a narrow sidebar.** When the full `↑v<latest>` doesn't fit the sidebar header — as happens as soon as the running version grows a digit, like `v0.4.10` — the hint now shrinks to a single `↑` (or `↻`) instead of disappearing, and that glyph is still the click that opens the update card. Only when even that doesn't fit does the hint go.
- **Person questions no longer depend on recency.** `backlot tell user` stays a report, `backlot tell user --ask` is the explicit question, and `ask.acknowledged` lets the user mark one seen without answering it.
- **A rate-limit stall stays one attention.** Reset-aware episodes escalate in place, self-continuing runners wait out their own reset, and Claude's weekly-limit paid-choice dialog is never answered by the stage.

### Changed

- **The Hollywood band shows one row per studio.** A READY row absorbs that studio's asks, other decisions fold into `+n`, and ASKS rows reply or acknowledge in place (`docs/studio/panel.md`, `docs/studio/ledger.md`, `docs/studio/limits.md`).

## 0.4.9 — 2026-11-26

A studio runs more than two actors at once: a new cast setting, `max_actors` (default 4), caps how many actors play together, the director is told a candidate is a kind of actor and to cast independent tasks in parallel, and `backlot status`, the panel and the Hollywood frames show `actors <live>/<cap>`.

### Added

- **A studio-wide actor cap, `max_actors` (default 4).** The cast caps how many actors play at once; only actors count — the producer, the director and the writer never fill an actor slot — and an actor being started counts too. A wanted actor past the cap waits `waiting for an actor slot` (distinct from a runner's `waiting for a <runner> slot`) and starts the tick one actor closes. It is part of the cast, so `backlot studio cast show|set`, `Edit cast…` and Hollywood `cast` cards change it; a value below 1 is refused. `backlot status` shows `actors <live>/<cap>`, and the panel and Hollywood frames show the same beside their cast.
- **The director casts in parallel.** The director prompt now says a candidate is a kind of actor (not a person), every cast starts a new actor with its own terminal, worktree and branch, one candidate can be cast to several tasks at once, and independent tasks should run in parallel — cast every task whose dependencies are met, up to the studio's actor cap, and let dependent tasks keep waiting. The producer's prompt asks for that split when it dispatches. Nothing in the code ever refused a second actor or capped it behaviourally; this makes the intent explicit (`docs/studio/guide.md`, `docs/studio/design.md`, `docs/studio/limits.md`, `docs/studio/ledger.md`).

## 0.4.8 — 2026-11-26

Change a studio's cast from the client: `Recast…` now works on actors, and `Edit cast…` on the studio menu opens the cast in your editor, shows what would change, and applies it on confirm. The same commands work over `--server`.

### Added

- **The cast changes from the client, and over `--server`.** The role menu's `Recast…` now sits on actor rows too, and the studio menu's `Edit cast…` opens the effective cast in `$EDITOR`, checks it with `studio cast set --dry-run`, shows the diff and records it on confirm. `studio cast show`, `studio cast set` (the file travels as content, never a path) and an actor `recast` all travel over the server queue when the client is on another machine. Also fixes a flaky `setsid -f` detach test by having its payload signal readiness (`docs/studio/guide.md`, `docs/keys.md`, `docs/studio/server.md`).

## 0.4.7 — 2026-11-26

A studio's cast is where it starts, not a lock: change its candidates, limits, checks, timings and role agents later with `backlot studio cast show|set`, recast an actor onto another agent, and let Hollywood propose `restart` and `cast` cards. The Hollywood band now reads as a conversation: a card or a silence shows under your line, the newest reply reads in full, and older turns fold into one row.

### Added

- **A studio's cast is a starting point, not a gate.** `backlot studio cast set --file <cast.toml>` changes a studio's roles, candidates, `[limits]`, checks and timings after it started — the cast file given to `studio new` now only initializes it; `backlot studio cast show [--json]` prints the effective cast as a cast file, and `--dry-run` shows the diff. Only what keeps the studio runnable is checked (a supported, installed runner, the effort and cline-model rules, a positive limit, caps that still seat the fixed roles). A fixed role whose identity moves is recast in the same apply. Live actors keep running, new limits apply to the next start, new checks run on the next delivery. `backlot recast` now also takes an actor key (`actor-<task>-c<n>`): the key, terminal and worktree stay, the conversation is new, and the task's deliveries and budget are untouched. Hollywood can propose these as cards: `recast` on an actor, the new `restart` and `cast` (`docs/studio/guide.md`, `docs/studio/hollywood.md`, `docs/studio/design.md`, `docs/studio/ledger.md`).

### Changed

- **The Hollywood band's talk shows a reply you can see, read in full, and the conversation that matters.** A card Hollywood proposes now draws as its own line under the person's line (`✦ proposed card c3 · acme-shop · pause t1`) and ends the live row like a report, so a recast that came back as cards reads as an answer where the person spoke; a card the stage proposed never joins the talk. An idle Hollywood whose newest `user ›` line still has nothing after it says `○ hollywood is idle and has not answered · open ↵` instead of going silent. The newest Hollywood line wraps to at most four rows at the band's width (two on a narrow/mobile band) with the Reader's CJK-aware wrap, so a long report's end — `choose 1`, `only you can publish: …` — is never cut; every other line stays one row. When the band is short the wrap gives its extra rows back before any waiting row is dropped. The talk keeps only the newest turn and the one before it, folding older lines into `… <n> earlier · ↵ read`, and the saved rows go to the waiting list (`docs/studio/hollywood.md`, `docs/studio/guide.md`, `docs/studio/design.md`).
- **The cast file no longer locks a studio, and an actor can be recast.** `backlot recast` accepts an actor key alongside the three fixed roles, and `backlot studio cast set` changes the cast after creation; what used to be refused as "not a fixed role" now names the task's current actor when the key is stale. The cast file given to `studio new` is the initial settings, not the only ones (`docs/studio/guide.md`).

## 0.4.6 — 2026-11-26

Use backlot from another machine: one server runs the studios and every other machine is a client. From a client you can start, finish, prune, delete and restore studios, browse the server's folders in the New Studio dialog, and approve cards, with every destructive step confirmed against the dry run you reviewed. Refused outward commands no longer count as waiting on you, a tell wakes a closed writer or director, an actor idling at its prompt keeps being pushed until it delivers, only you finish a studio, and Devin's SWE-2.1 models are in the cast picker.

### Added

- **Devin's SWE-2.1 models are in the cast picker** (`swe-2-1-max` … `swe-2-1-medium` and their Ultrafast
  variants). The default Devin model stays `swe-2-max`.
- **Use `backlot` from another machine (`--server`, `BACKLOT_SERVER`, `[client]`).** Both the TUI and CLI now accept `--server <host>[/<session>]` (default session `backlot`), `BACKLOT_SERVER`, or `[client] server` / `session` in `~/.config/backlot/config.toml` so you can view studios and run person actions against a hub server (`backlot-server`) with no local stage while leaving everyday local terminals unchanged. Off-hub person actions submit typed requests through `plugin.backlot.req` and wait for acks in `plugin.backlot.res` (reporting `server busy`, `request expired`, timeouts, or `not available through the server yet` for unsupported studio lifecycle verbs); CLI calls run on the hub itself still write the local ledger directly so they keep working across server restarts, and when the server terminal is stopped while `backlot-cat` is present, the TUI shows the command to start it without spawning anything itself (`docs/studio/guide.md`, `docs/studio/requests.md`, `docs/studio/panel.md`).
- **Adaptive Lot Reader with sibling stepping, raw OSC 52 copy, and inline `t` composer.** Reading any studio `✦` / `›` exchange row, Hollywood headline/report, actionable card, actor `records`, or Task & Delivery Sheet report/log opens an adaptive content-sized card centered in the modal surface with structured header metadata (additive optional `to`, `kind`, `task`, and `ts_ms` fields on `backlot/1` summaries), display-width-accurate CJK/URL/Markdown/log wrapping, and an undimmed beside Lot Dock whose `▌` cursor follows `←`/`→` (`[`/`]`) sibling stepping in place. Action strips adapt across width tiers (`[t] tell`, `[o] focus/reopen`, `[p] peek`, `[v] sheet`, `[y] copy`, card `[a] apply` / `[x] cancel`, `? keys`) while keeping `y` copy-only and `d` unbound so prompts are never answered by accident; pressing `t` or `i` opens an inline composer sharing drafts and `↑`/`↓` history (plus studio `Tab` `/verb` completion) directly with the Lot Dock or Call Sheet (`docs/keys.md`, `docs/studio/panel.md`, `docs/studio/hollywood.md`).
- **Single machine-wide server mode (`backlot server run|start|stop|adopt|service`).** Runs the existing daemon loop directly under `~/.local/state/backlot/server/` with an exclusive `server.lock` (`flock`) and renewed `server-lease.json`, blocking duplicate local `server run` instances and local `stage start|run` processes across sessions while routing local studio and Hollywood commands to `server/`. `server adopt [--dry-run] [--stop-stages] [--core <id>]` promotes an existing `<core_id>` state directory into `server/` with relative symlinks for live git worktrees, archives secondary/empty cores under `server/adopted-<stamp>/`, and leaves `server-pointer.json` tombstones (`managed by backlot-server (--session backlot)`); `server service [--print | --write]` generates the `Restart=always`, `RestartSec=2` user unit; and `studio new --remote` refuses with the Phase 2 placement message (`docs/studio/server.md`).
- **Policy-driven outward pushes, `CardAction::Outward` cards, and Call Sheet / Dock controls (Phase 1).** Feature-branch pushes to recorded studio remotes now pass under the default `recorded` push policy (`[push]` in cast/user config, with `any` and `none` modes, `protected_branches`, `allow_remotes`, `allow_tags`, and `allow_gh_pr`). Protected refs, tags, unrecorded remotes, and `gh pr create` are classified as cardable (`GuardVerdict::Card`), while force pushes, `--mirror`, unknown push transports, `core.*`/transport config overrides, and non-PR writes remain refused (`GuardVerdict::Refuse`), with shell/alias hardening across `guard`, `guard_shell`, `agy` `PreToolUse`, and `cline` POSIX `PreToolUse.sh`. Roles can also explicitly request carded outward execution via `backlot ask --run [--why <reason>] -- <command>`, validated for work-area containment, clean local git config, and pushed `HEAD` precondition on `gh pr create`. Stage proposes and executes applied `Outward` cards asynchronously (`outward/c<n>.log`, 120 s timeout with TERM→KILL escalation) in a clean person environment (`BACKLOT_*` and `GIT_CONFIG_*` stripped, guard shim dirs removed from `PATH`), re-verifying policy/git hygiene/staleness before spawning and delivering `"outward"` notices (`Succeeded`, `Failed`, `TimedOut`, `Denied`, `Stale`) back to the requesting role. Process-tree ancestor walking (`BACKLOT_TERMINAL`) guards `backlot card`, `--as user`, and the Phase-1 `perms revoke` stub against unguarded role invocations. `StudioSummary.outward_cards` surfaces open outward cards in both the Lot Call Sheet and Studio Dock with or without `[hollywood]` (deduped locally against `HollywoodSummary.cards`), supporting `a`/`y` (`card apply`), `x`/`d` (`card cancel`), `s`/Enter (`card show`), and remote hosts over `RemoteRunner`.
- **Start and end studios from another machine.** `studio new`, `finish`, `prune`, `delete`, `restore` and the New Studio dialog's `repos` / `runners` probes now travel over `plugin.backlot.req`/`plugin.backlot.res` like the other person verbs. The cast and the first message (`--cast [file|-]`, `--ask`, `--ask-file`) are read on the client and sent as content; the hub validates and canonicalizes `--repo` against its own filesystem and runs the same code as the local CLI, whose `--json` documents and ack results are now one struct. `studio delete` and `studio prune` (and `studio restore --name`) are reviewed: a dry run returns `{ report, confirm_token }` (`sha256` of the report's canonical JSON), the commit must echo it, and an unreviewed or stale commit refuses with `confirm this from a dry run first` or `the studio changed since you reviewed it; run it again`. Hub-local calls keep writing the ledger directly with no token, and `--remote` placement stays Phase 2. The server-connected TUI drives the same queue: its New Studio dialog probes the hub's version, folders and runners and sends the cast on stdin with the build brief as content, its delete/prune/restore dialogs keep the plan's `confirm_token` for the confirm, and a stale plan is shown in the dialog with `r` to review again — no client temp file, no local `stage start` (`docs/studio/guide.md`, `docs/studio/server.md`, `docs/studio/requests.md`, `docs/studio/panel.md`).

### Changed

- **Only you finish a studio.** `backlot studio finish` is the person's alone: a role that runs it — the producer included — is refused with `finishing the studio is the person's call; say in your report that it is ready, and they finish it from the lot`, and nothing joins the ledger. The producer reports that the studio is ready and what is still yours to do; the studio menu in the lot finishes it (`prompts/producer.md`, `docs/studio/guide.md`, `docs/studio/ledger.md`, `docs/studio/design.md`).
- **A refused outward command is informational, not the person's.** `backlot guard` now raises its `outward action` attention with `needs_user: false`, so a refusal the role's own turn moves past no longer counts toward the person's needs-you count, the lot pill or notifications — it stays on the studio's attention list and the role's row as `info`. The role's refusal line still tells it to `backlot ask` when it really needs the command, and the attention clears as `role moved on` as soon as the role writes anything else to the ledger after it (its next `progress`, a `tell`, or a later refusal), as well as once the stage delivers it new work. Reasons where only the person can act — a permission or trust prompt the stage will not answer, a user-addressed question, `cannot resume`, launch/prep failures, `over budget`, `agent error` and the escalated `owed` step — stay `needs_user: true`. Carded pushes and `gh pr create` are unchanged.
- **A tell wakes a closed writer or director.** A `backlot tell` to the writer or director while it is closed because it is not wanted now starts it again with the message delivered, the way a `plan` request does, unless the studio is finished: a delivered-pending tell keeps the role in the wanted set until the role answers it (its next ledger write, such as a step-boundary `progress`, or a `tell` back). A tell to an actor whose task is finished or abandoned is refused with a line saying so, instead of queuing mail nobody will read.
- **The selected studio stands out with an accent border, title chip, and bold strip/stacked label.** Whether or not the lot owns the keys, the selected studio (cursor studio, else `▸` current studio, else first studio) draws all frame edges (`╭ ─ ╮ │ ╰ ╯`) in `accent` with a bold high-contrast `accent` title chip (` ▸ <studio> `) in its top border, while unselected blocks stay muted in `surface1`; collapsed strip and stacked rows also highlight the selected studio's label in bold `accent`.
- **An actor that stops at its prompt keeps getting pushed until it delivers.** When the owed step is an actor's delivery on a running task, the nudge now names the task and says keep working until `backlot deliver`, do not end the turn with a progress line, and send a block to the reviewer with `backlot tell <reviewer> --file <file>`. If the actor answers only with a `backlot progress` line and goes idle again, a second, firmer nudge follows once the idle stretch passes `nudge_after_s` (every other step still gets one nudge ever). For that step only, a bare `role.progress` no longer counts as activity for the `owed` escalation or the `role active again` clear — any other ledger write, a commit or a work-area change still does. `backlot status` and the TUI role row show `idle <n>m` for a live role that has sat at its prompt longer than 60 s instead of plain `waiting` (`docs/studio/design.md`, `docs/studio/guide.md`, `docs/studio/panel.md`).

### Fixed

- **A cmd session created in the same clock tick as its role start is still found.** Session discovery now accepts a session file born up to 2 s before `started_ms` — the kernel's coarse file clock can stamp a file written just after the launch earlier than the start record — for `cmd`, and the same slack covers `agy`, `cline` and `codex` discovery; the `taken` set and the cwd check still decide which session is the role's.

### Security

- **Remote clients approve cards over the server queue; a role terminal stays a seatbelt away from your own controls.** Off-hub `backlot card apply|cancel` (CLI and TUI `a`/`x`) travel over `plugin.backlot.req`/`plugin.backlot.res` like the other person verbs. A role terminal cannot reach your queue, socket, server or a detached process: `hc run|exec|send|keys|connect` (reads stay allowed), interpreter code (`python -c`, `node -e`, a heredoc) that writes `plugin.backlot.req` or `doc.set` or reads the core socket, the detach tricks that leave the role's process tree (`systemd-run`, `tmux new -d`/`new-session -d`), and `backlot server run|start|stop|adopt|service` are refused by the PATH shims and by the `agy`/Cline command-line guards, with `hc run --bg 'backlot card apply c1'` refused outright. Ordinary commands keep working even when their words name the queue or the socket directory (`grep doc.set`, `cargo test -p herdr-core`). Roles run as your own Unix user, so this is a seatbelt against a confused or overeager agent, not isolation (`docs/studio/guide.md`, `docs/studio/requests.md`, `docs/studio/design.md`).

## 0.4.5 — 2026-11-25

The lot stays short: only the studio you are in shows everything, and closed seats and done tasks fold into one line each. `Alt+L` jumps to the lot, and backlot is now based on hc-tui v0.1.20.

### Added
- **`Alt+L` (`[keys] focus_lot`) focuses the lot on its last selectable row.** Pressing `Alt+L` opens/expands the right-hand lot dock (`lot · N`) if hidden or collapsed, focuses it, and scrolls the selection to its last selectable row (e.g. `› tell producer · / actions`) without entering Compose (`i`/`/` composes, `↑` walks up, `Esc` leaves). Pressing `Alt+L` while the lot is already focused jumps to the last row again without toggling the dock closed. Configurable via `[keys] focus_lot` (empty string unbinds), falls through to the focused program under `--plain`, yields to floats, modals, the Call Sheet, and running compose, and appears in the lot header, `?` help, launcher, and docs (`docs/keys.md`, `docs/studio/hollywood.md`, `docs/studio/panel.md`).
- **Core-document request queue, acks, and catalog (`plugin.backlot.req`, `plugin.backlot.res`, `backlot-cat`).** Added additive `EventBy.via` (`"doc"` for document-driven person actions), typed request/result/catalog schemas in `backlot-model`, shared typed person-verb handlers in `backlot-stage`, CAS client helper (`doc_client`), and stage consumer/publisher wiring with immediate `doc`-event wake plus startup/60s poll, 60s request expiry/future-skew checks, 24h persistent dedupe (`processed-reqs.jsonl`), strict size caps (32 queue / 64 acks / 64 KiB result), and hard refusals for `approve`/prompt/outward/non-person inputs and interactive prompt clearing (`docs/studio/requests.md`).

### Changed

- **Synced with hc-tui v0.1.20:** a shown float takes every key except its popup chords and `PageUp`/`PageDown` page its own scrollback, focus moves to a sibling pane when a pane's program exits, unnamed workspaces follow the focused terminal's live folder (and new workspaces work on an empty core), closed `herdr-app` spaces and tabs stay hidden, child terminals ended with `die_with_parent` auto-reap, `[ui] agent_panel_space_headers` adds optional space headers, and official `hc` compatibility aligns with 0.1.22 (`7cb4de47`).
- **The lot folds unselected studios and rolls up closed seats and done tasks.** In lots with two or more studios, only the selected studio (where the dock cursor sits, else the `▸` current studio, else the first studio) renders its full block; every other studio folds to its frame and live roles, falling back to stacked one-line rows when cramped, and moving into or clicking a folded studio unfolds it cleanly without jumping the cursor. Inside a full block, closed seats fold into one dim `□ <n> closed · <names>` line (first-seen order with `×k` counts, toggled per studio by `Space`/`Enter`), and accepted tasks fold from 2 tasks on as `✓ <n> done · <ids>` (toggled by `Space`/`Enter`).

### Fixed

- **Plain `backlot` folders and git checkouts stay visible and reachable with `Ctrl+K` / `Ctrl+J`.** `is_backlot_stage_space` now matches only the internal `herdr.space = "backlot"` space (`space:7:backlot`) instead of every workspace whose label is `"backlot"`. Previously, any plain shell in a `backlot` directory or git checkout disappeared from the left `machines` tree and from `previous_workspace` / `next_workspace` cycling in studio mode.

## 0.4.4 — 2026-11-25

"Needs you" now means it: a studio row stops asking for you once the role it was about has moved on.

### Fixed

- **Stale "needs you" rows clear by themselves.** An `owed` escalation clears (`role active again`) once its role writes to the ledger again, for example while the person talks it through, and raises again only after the role sits idle a whole `escalate_after_s` with the step still owed, counted from its last ledger write. An `outward action` refusal clears (`role moved on`) once the stage delivers the role new work. Before, both stayed "needs you" until the step was done or someone cleared them by hand.

## 0.4.3 — 2026-11-25

Hollywood can change its own agent with one card. When you ask it to switch to another runner or model, it proposes a `setup` card showing the old and new identity; applying it rewrites `[hollywood]` (keeping `start`) and restarts Hollywood once, and the new Hollywood greets you with what changed instead of re-researching. The Call Sheet title row's `m` menu also offers "Change Hollywood's agent…" directly.

### Added

- **Hollywood self-setup card and Call Sheet title menu.** Added `CardAction::Setup` (`backlot hollywood propose setup --runner … --model … [--effort …] --why …`), validated at birth like `hollywood setup` against a `HollywoodIdentity` base and displaying `Hollywood: <old> → <new>`. Applied cards write `[hollywood]` while keeping `start` and all other keys, record `card.done` before requesting one fresh restart, and start the new Hollywood with a takeover first prompt (`re-cast from <old> → <new>`), plus a `card.done` passive outcome note (`Hollywood re-cast to <new>`). Added the Call Sheet title row `m` menu (`Change Hollywood's agent…`) opening the prefilled `Recast…` runner/model/effort picker directly via `hollywood setup`. Updated `prompts/hollywood.md`, `prompts/hollywood-cli.md`, `docs/studio/hollywood.md`, and `docs/keys.md`.

### Fixed

- **`hollywood setup` preserves `[hollywood].start` and detects model/effort changes.** Rewriting `[hollywood]` now keeps `start` and all other keys (instead of reconstructing the table from scratch) and lists `agy` among supported runners; `hplan` now triggers a fresh restart when Hollywood's model or effort changes on the same runner.

## 0.4.2 — 2026-11-25

This release reworks how the person and Hollywood talk. The person's messages now reach Hollywood first and inline,
ahead of stage notices, which are fewer: no notice for an applied card, and clean outcomes ride along as context instead
of waking Hollywood. Alt+Y opens the Call Sheet ready to type, and every `›` line keeps an input history on Up and Down.
A cast role that has not started yet, such as a recast writer waiting for a plan, now shows as standby.

### Changed

- **Hollywood two-lane inbox & notice hygiene.** Hollywood inbox now delivers user messages first
  and inline (merged in order with a deferred-notice footer), filters closed/`info`/duplicate card
  notices, supports `--answers` (`message.seen`) to prevent re-delivering answered messages, and
  refreshes `AGENTS.md` on adopt.
- **Call Sheet `Alt+Y` Compose landing, universal `›` history, `Esc` resting, and 3-zone `Tab` ring.** Call Sheet `Alt+Y` now opens straight into Compose from Hidden or Collapsed, adds shell-like `↑`/`↓` message history to every `›` line (Call Sheet and Studio Dock), uses `Esc` to leave Compose, and cycles `Tab` across Compose → Waiting → Studios.

### Fixed

- **Show unstarted fixed roles as `standby`.** Fixed seats cast by a studio (`producer`, `director`, and `writer` before the director asks for a plan) are now listed in their usual summary slot with `state: "standby"` and their effective recast or cast identity instead of being absent before their first start. Both the cast tree and the dock render `standby` rows dim with no activity glyph or counts and a hint line (`starts when the director plans` for `writer`, `starts with the studio` for `producer` and `director`), explain why `p` and `Enter` are unavailable, keep `Recast…` active in the `m` menu and `Ctrl+B ?` launcher, and report `writer starts when the director asks for a plan` on `backlot recast` and Hollywood recast card outcomes when the role has not started yet.

## 0.4.1 — 2026-11-25

This release swaps the studio and role keys, so `Alt+Left`/`Alt+Right` step between studios and `Alt+Up`/`Alt+Down`
between a studio's cast, and stops counting a role's question as waiting on the person once the person has answered it
in the role's own terminal.

### Changed

- **Swapped `Alt+arrow` studio navigation defaults.** `Alt+Left`/`Alt+Right` now step studios (`previous_studio = "alt+left"`, `next_studio = "alt+right"`) and `Alt+Up`/`Alt+Down` step cast members inside a studio workspace (`previous_role = "alt+up"`, `next_role = "alt+down"`); to restore the old layout, set `previous_studio = "alt+up"`, `next_studio = "alt+down"`, `previous_role = "alt+left"`, and `next_role = "alt+right"` under `[keys]` in `~/.config/backlot/config.toml`.

### Fixed

- **An ask answered in the role's terminal stops waiting.** When a role asked the person with `backlot tell user` and the person answered by typing into the role's terminal, the panel kept `ASKS`, `? asks you` and the needs-you counts. The stage now records `ask.answered` when the role starts a turn the stage did not cause (a prompt hook signal, or `waiting` → `working` for runners without one); the question stays in the exchange. A person's recast no longer clears an ask.

## 0.4.0 — 2026-11-24

This release lets agents that never report progress serve as studio roles. The stage now raises a stall only when a role
shows no sign of life at all: no hook signal, no screen change, no busy child process, no work area change and no
ledger event. A dialog on a role's own screen is escalated, never answered by the stage, and an outward action such as a
push, `gh`, a deploy or an install is refused and escalated to the person. agy, cmd and Devin get hook signals. A role
may report one line with `backlot progress`, and the panel shows it next to the command the role is running. Around
that, studios gain lifecycle actions (restart or recast a role, free work areas, delete with an archive and restore),
notifications wherever the person is looking, one needs-you list, each studio's next step, and a keyboard-driven left
tree.

### Added

- **One-line role progress & live descendant command display (D5 / R7e).** Added
  `backlot progress "<line>"` (`--file`, `--text`, `--stdin`) recording `role.progress`
  (control chars stripped, collapsed to one line, capped at 200 chars) in `backlot-model`
  (`progress.rs`, `RoleProgress`, `RoleRunning`, `EventKind::RoleProgress`, `HollywoodKind::RoleProgress`)
  and `backlot-stage` (`args_studio.rs`, `commands.rs`, `cli.rs`, allowed across all role prompts
  and runner base rules; returns `Fail::Usage` naming `BACKLOT_ROLE`/`BACKLOT_STUDIO` outside a role env,
  with no stall/nudge obligations). Extended D4 process-tree probing (`liveness_diff.rs`, `liveness.rs`,
  `live.rs`, `daemon.rs`) to track `deepest_descendant` `argv` (capped at 60 chars) under
  `RoleSummary.running` / `LiveRole.running` / `RoleState.running` only while `work == working`;
  `RoleSummary.doing` and `LiveRole.doing` stay unchanged (`Option<String>`) for older clients.
  Updated `backlot status`, `backlot status --json` (per-role `progress` and `running`, once each),
  `hc-tui` panel (`panel/working_line.rs`, `PersonLine::Working`, client wall-clock ages via
  `update_state::now_ms()`), and studio docs to show `working <dur> · "<progress>" <age> ago · <running> <dur>`
  with progressive narrow-width fitting.

- **Unified six-source liveness evidence replacing separate stall clocks (D4 / R7d1).** Added
  `backlot-stage/src/liveness.rs`, `liveness_probe.rs`, `liveness_diff.rs`, `liveness_work.rs`, and
  `runners/normalise.rs` (+ `normalise_strip.rs`), plus `RoleLastEvent` / `last_by_role` in
  `backlot-model`: one `Evidence` track per `(studio, role)` follows hook signals, mid-turn
  `activity.revision` (`revision_stalls`), per-runner normalised screens (stripping spinners,
  timers, `Thought for …` headers, token counters, and status clocks while keeping the send gate on
  raw screens), local process trees (`term.pid` via `/proc` on Linux or `ps` CPU-only on macOS,
  10 s sample throttle, root CPU/I/O floors of `8 jiffies/s` and `16 KB/s` measured on `agy` and
  applied only without session hook signals, and exited `cutime+cstime`), local git work areas
  (`HEAD`, resolved ref, `.git/index` mtime, and 60 s `git status --porcelain` hash), and role
  ledger events (`by.role`). Kernel inode lock waits (`lock_inode_wait`) get a `3 × stall_limit_s`
  threshold (`waiting on: <argv> for <dur>`); pure screen animations stall with
  `screen animates only (spinner)`; any moving evidence clears `stalled` naming its source. Added
  `spin:` to `tools/fake-runner.sh` for spinner harness tests.

- **Signal & guard hooks for `agy`, `cmd`, and `devin` (R7d2).** Added `--runner <name>` to
  `backlot hook` normalizing `agy` (`conversationId`, `hookEventName`, `PreInvocation`/`PostInvocation`)
  into `signals.jsonl` and enforcing `guard_rules` on `agy` `PreToolUse` (`run_command`
  `CommandLine`) with `role.refused`, deduplicated `outward action` attention, and a 1.5 s fail-open
  budget returning `{"decision":"allow"}` / `{"decision":"deny","reason":...}`. Wired per-work-area
  byte-preserving hook files on every start and resume for `agy` (`.agents/hooks.json`), `cmd`
  (`.commandcode/settings.json`), and `devin` (`.devin/config.local.json`), excluding new files in
  `<git>/info/exclude` (following linked-worktree `commondir`) and leaving tracked files untouched with
  a `hooks unavailable` Info attention and launch log. Updated `caps.rs` (`agy`, `cmd`, `devin`
  `hooks`, and `devin` `prompt_detection.hook = true`) and regenerated `docs/studio/runners.md`.

- **Dialog escalation, `agy` work states, and turn-level revision stalls (R7b).** Disabled
  structured `activity.revision` stall tracking for runners whose revision only bumps on turn
  completion (`mid_turn_revision: false` for `agy`); wired `agy` 1.5.16 work states, prompt
  blocks, and `Native` status caps from R7a; added D2's 12-line generic dialog fallback rule in
  `runners/dialog.rs`; raised, routed (`actor` → reviewer, `director`/`writer` → `producer`,
  `producer`/Hollywood → `needs_user`), and cleared (`dialog closed`) `question` attentions for
  non-permission dialogs while keeping `gate_open` closed; and denied Claude `AskUserQuestion`
  while instructing role prompts to use `backlot ask` / `backlot tell`.

- **Outward-action guard (`backlot guard`, shims, git hooks, and `agy` deny rules).** Every launched role (including Hollywood) gets `<role>/bin` first on `PATH` (ahead of `exe_dir`) with `gh`, `hc`, `scp`, `rsync`, `ssh`, `npm` and `cargo` shims, plus `GIT_CONFIG_*` `core.hooksPath` pointing at `<role>/git-hooks` (appended without clobbering existing `GIT_CONFIG_*`, refusing `git push` via `pre-push` while passing client git hooks through to the repository's own hooks). Refused commands exit 126, record `role.refused` in studio ledgers and raise one `outward action` attention (`needs_user: true`) per episode. `agy` launches also splice the 14 `command(<prefix>)` entries (without trailing ` *`) into `.agy-settings.json` `permissions.deny`.

- **The lifecycle actions in `Ctrl+B ?`.** A `lifecycle` group lists `Restart <role> ·
  <studio>`, `Recast <role> · <studio>`, `Free work areas · <studio>` and `Archived studios ·
  <machine>`, filterable by role, studio or machine name.

- **Archived studios in the left tree.** When a machine's `studio list --archived --json` is
  non-empty the finished group gains an `Archived (n) …` row; Enter lists each entry (name,
  stamp, repo, requirement) and `Restore as <name>` runs `studio restore` with `--dry-run`
  first, then for real. The archived list is cached per machine like the ledger list.

- **`Free work areas (prune)…` and a finish guard.** A finished studio's menu fetches
  `studio prune --dry-run`, shows its plan, and then prunes for real. `Finish studio…` on a
  studio with no requirement and no tasks asks `Nothing was built yet (no requirement, no
  tasks). Finish anyway?` and passes `--force`.

- **Restart and recast from the role menu.** A live or stuck role's menu offers `Restart <name>`
  — a two-choice confirm, `Resume its session` (the default when its runner keeps a resumable
  session) or `Fresh start`, running `backlot restart --as user` on the studio's machine.
  Hollywood's own menu carries `Restart Hollywood`. The fixed seats — producer, director, writer
  — add `Recast…` (runner, then model, then effort from `backlot runners --json` and each
  runner's caps; a no-effort runner has no effort step, an optional one allows empty) and no
  longer offer the `Close` that the command refuses on them.

- **Studio events notify the person, wherever they are looking.** The client now derives
  notifications from the `backlot` documents, not only from pane status words: a role's
  unanswered question (`producer asks: <question>`), a user-owned prompt on a role or on
  Hollywood (`hollywood needs you: <command>`), a Hollywood reply
  (`hollywood replied: <headline>`), an open card (`card waiting: <action>`), a studio every
  task is done on (`acme-shop is ready to finish`) and a stage that stopped with live studios
  (`stage stopped on host-a`). Each event rides the existing settle → queue → TTL pipeline with
  the same `[ui.sound]` and `[ui.toast]` config, fires once per ledger seq (the first document
  after start seeds silently, and a reconnect reseeds), and is suppressed when its target is
  already focused, exactly as a watched pane is. The toast's key (`prefix+o`) or a click opens
  the row's target — the role's terminal, `bl-hollywood`, the Call Sheet on that card, the
  studio, or the dock's `stage stopped` row (selected, never started: `s` stays the person's
  press). A new `[ui.toast] studio = false` (default `true`, backlot-only) turns them off;
  `--plain` emits nothing.

- **The lot shows who runs on what.** Every person row in the studio dock now carries a
  dim runner tag before its state word (`claude`, `cmd`, `agy`, `codex`, `devin`,
  `cursor`, `cline`, `opencode`, `fake`) and, while the frame has room, the cast's short
  model form. The state word keeps the right edge and the name and task keep their cells:
  the badge uses only spare space, dropping the model first, then the tag, so the name
  truncates only when it alone overflows.

- **"Restart to update".** When the local stage's published `backlot_version` (with its
  published `backlot_build`, compared once the versions are equal) or an installed
  `backlot --version` from the six-hour machine probe is newer than the running client,
  one dim line closes the lot: `backlot <new> installed — restart the client to update
  (q, then backlot)`. Strict semver decides; an older stage never draws it. The build id is
  compiled in from git (`HEAD`, `-dirty` while the tree has changes), and
  `backlot --version` prints it as `backlot 0.3.0 (abc123def456)` so equal-version rebuilds
  are still distinguishable.

- **The left tree answers to the keyboard: the launcher's machine rows.** `Ctrl+B ?`'s
  `machines` group now carries, per machine, `Stage log · <machine>`,
  `Unfold finished studios (n) · <machine>` (or `Fold …`) and `Machine menu · <machine>` — the
  latter opens the same menu a right-click does — beside the existing stage rows, all filterable
  by machine name.

- **The left tree answers to the keyboard: focus, movement and row actions.** `Ctrl+B t` moves
  the keyboard into the left machines tree (opening the sidebar if it is collapsed) and
  highlights the row for the current place; `↑`/`↓` (and `k`/`j`) move the highlight and the
  section scrolls to it; `Enter` runs the same action a left-click does — a workspace row focuses
  its terminal, a stage-down studio starts the machine's stage, the `finished (n)` fold toggles,
  a finished studio opens its menu, a machine row folds its section — and `m` opens the row's
  context menu. `Esc` returns the keys to the terminal. `Ctrl+B t` is free in the default keymap
  (`Ctrl+B Tab`/`Ctrl+B h` would collide with the pane cycle/focus bindings).

- **The empty states say what to do.** With every stage down and no published studio, the lot
  now says `<n> studios on stopped stages — s to start` (starting that one stage when a single
  machine holds them all) instead of `no studios`, and the cast says `stage stopped — s to start`
  instead of drawing blank.

- **No truncated ids in the tree's stage-down row.** The tree's stage-down row puts the studio
  name first and shortens its tag to `down`, so a 10–12 character id fits whole at the default
  sidebar width instead of reading `beta-too…`.

- **A new studio starts with what to build.** `backlot studio new --ask <text>` (or
  `--ask-file <f>`) queues the person's first message to the new producer as the same
  `message.queued` a `backlot tell producer --as user` records, so the stage delivers it to the
  producer's terminal as its first `[backlot] user → producer: read <file>` line. An empty ask is
  refused and records nothing; `--dry-run` restates its first line. The New Studio dialog's step 2
  gains an optional **What should this studio build?** field — multi-line, `alt+⏎` adds a line,
  `Ctrl+E` edits it in `$EDITOR`, empty is today's behaviour — whose first line shows on step 4's
  summary card under `First ask`; a non-empty brief rides the create as `--ask-file`, written to a
  temp file on the target machine. The producer's prompt now reads a first `tell` from the user as
  the requirement brief and records it before asking "What would you like us to build?".

- **Every machine's studios in the left machines tree.** A machine whose stage is down — or that
  published no document — now lists its studios from `backlot studio list --json` (run locally or
  over `RemoteRunner`, cached and refreshed on a stage change or every 30 s at most), dimmed with
  a `stage down` tag; Enter or a click on one starts that machine's stage. Each machine also
  carries a folded `finished (n)` row under its live studios that unfolds with Enter/Space (or
  the machine menu's `Unfold finished studios (n)`), and a finished row opens the existing
  finished-studio menu. A machine that cannot be reached shows one `unreachable` line instead of
  an empty tree, and the tree scrolls to the machine row of the studio the selection lands on,
  so a row never drops out of a short sidebar. The cast explains itself: with no studio current
  it shows the only live studio's cast, or one dim `select a studio (Enter on it)` line. Starting
  the stage keeps the focus where it was (from the opening page it lands on a studio or the lot,
  never the stage tab); the stage's terminal is read-only to keystrokes, its bar reading
  `stage log — read only`, and the machine menu's `Stage log` opens it as the observe-only peek.

- **Restart a stuck role in place.** `backlot restart --person <role> [--fresh] [--reason <r>]`
  reloads a live or lost producer, director, writer or actor under the same identity: the stage
  stops its terminal deliberately (`role.closed`, reason `restarted` — never a loss) and launches it
  again, resuming the recorded session when the runner supports it, or starting a new one with
  `--fresh`. The role's ledger row carries `restarted`, which the summary and Hollywood read. The
  same request reloads Hollywood itself (`restart --person hollywood`, its own `role.restart`).

- **Archive list and restore.** `backlot studio list --archived` lists the studios `studio delete`
  retired — name, stamp, repo and last requirement title — and `backlot studio restore
  <archived-name> [--name <new-name>] [--dry-run]` copies one back under `studios/` as a finished
  studio, refusing a name already taken. `docs/studio/guide.md` gains a finish/prune/delete/restore
  table.

- **Hollywood's terminal starts at a readable size.** `bl-hollywood`'s `terminal.run` now carries
  an initial grid of at least 100 columns and hc's `200×120` default, so a person who peeks or
  focuses it sees its answers instead of a 21-column strip. Peeking stays
  strictly read-only: a peek attaches as an observer, which never sends a size, so no client view
  can narrow it — only focusing it into a pane does (H3).

- **Hollywood's AGENTS.md carries a generated CLI reference.** The stage writes one line per
  command Hollywood may run, with its flags, sliced from the CLI's own usage strings
  (`args_usage.rs`, `args_hwood.rs`) at every start, and substitutes it for `{{CLI_REFERENCE}}`
  in the prompt. A drift test regenerates the checked-in copy (`prompts/hollywood-cli.md`) and
  fails when a usage string changes without it. The prompt says to use the reference and run
  `--help` only for a command it does not list, so Hollywood never probes its own CLI or blocks
  on a prompt for it (H2, P5).

- **Hollywood is fed facts, not snapshots.** Every attention Hollywood can read — in
  `backlot status`, in a notice body, in the status commands it runs — now carries its class
  (`needs you` / `stuck` / `info`), its age (`raised 2h ago`) and, for a screen snapshot, when the
  screen was taken (`screen at 14:02, may be stale`). Each role line also carries its current live
  work state (`working`, `waiting`, `blocked`), read from `<state dir>/live.json`. The stage writes
  that file on a change and a heartbeat (at most every 5 s), and a reader drops a file older than
  15 s — after the stage stops, `backlot status` says `stage not running — live state unknown`
  instead of the last `· working`. Before this, Hollywood read a launch-time sign-in snapshot
  as the producer's current state and reported a healthy producer as "not signed in". Its prompt
  now says: decide from the role's current state, never from an old snapshot, and never report
  `info` bookkeeping as a problem. `AttentionClass::label` is the one wording every surface reads.

- **Hollywood's bar shows what it is doing, and asks you when it needs permission.**
  The summary's `hollywood` object now carries `doing` (the terminal's reported
  status, read and capped like a role's), and — while a permission prompt stands on
  `bl-hollywood`'s screen — `attention`, `attention_user` and `attention_detail`
  (the command the picker asks about). The expanded Call Sheet draws one live row
  under the newest `user ›` line while that line has no reply after it: `… sending
  to hollywood` while the message is still in the outbox, `⠶ hollywood: <doing>` (or
  `⠶ hollywood is working`) while it works, and `⚠ hollywood asks: <prompt>` at a
  prompt. The prompt row takes `y` allow once, `d` deny, `p` peek and Enter to focus
  `bl-hollywood`; `y`/`d` run `backlot approve --person hollywood --decision …`,
  which records `role.approval` in Hollywood's own ledger. The stage sends the
  runner's one-time key (`enter` on cmd's `❯ 1. Yes`), never "don't ask again", and
  only while the prompt the decision answered still shows. Hollywood's cmd launch
  also pre-allows the read-only `backlot --help` and `backlot help`, so it can read
  its own CLI without a prompt.
- **One attention class per reason.** `backlot_model::attention::AttentionClass::of` sorts every
  reason the stage can raise into `needs you`, `stuck` or `info`: a person's prompts, refusals,
  failed launches and budget overruns are theirs; a stall with nudges left, a rate limit or a
  connection error is `stuck` until the stage escalates it; `session id not found`,
  `inherited XDG_CONFIG_HOME` and `cursor dir was open` are stage bookkeeping. An unrecognised
  reason is `needs you`, so nothing important is hidden. The role summary carries the class as
  `attention_class`, and `docs/studio/panel.md` holds the full table.
- **One honest state per person, in plain words.** A person row now shows exactly one state, in
  this priority: `needs you · <what>`, `asks you · <question>`, `stuck · <reason>`,
  `working · <doing>`, `waiting`, `ready`. `info` attentions — the stage's own bookkeeping such as
  `session id not found` — never change the state; they show only in the role's drawer and the
  sheet's overview, dimmed. The old `stalled` label is gone: a `stuck` episode reads `stuck …`,
  peach, and a needs-you attention that is not a prompt reads `needs you · …`, red. A live
  producer with `work: waiting`, `doing: idle` and an open `session id not found` now reads
  `ready`, not `! stalled`.
- **The producer's questions reach the person.** The producer's prompt now says every question to
  the user — including the first "What would you like us to build?" when no requirement is
  recorded — also goes through `backlot tell user --text <question>`, while the terminal stays the
  conversation. The role summary carries the newest unanswered `tell user` as `asks`, which the
  person row shows as `asks you · <question>`; a user `tell` back to that role — or any later
  ledger action the role records itself, such as a revised requirement, report or delivery —
  answers it, while stage-written events do not.
  `backlot tell user` is covered by every runner's pre-allow list.
- **One needs-you list everywhere.** One function builds it from every machine's `backlot`
  document — user prompts, roles with a needs-you attention or an unanswered question, Hollywood
  with a user-owned prompt (`HOLLYWOOD asks: …`, taking the same `y` allow once / `d` deny /
  `p` peek / Enter as its live row) or a blocked screen without one, studios with no requirement,
  stopped stages, open cards and ready studios. The Call
  Sheet's rows, the lot pill's urgent count and the band's calm row read it, so
  `✓ nothing waiting on you` shows only when the list is empty; the misleading `!N flagged`
  suffix is gone, and a `stuck` role shows on its own row.
- **Every studio shows its next step.** The dock's studio row and the Hollywood frame carry one
  `next:` line from the studio's own state: `tell the producer what to build` when no requirement
  is recorded (Enter on the studio row or `t` opens the producer tell input), `director reviewing`
  while tasks are in review, and `finish (f)` once every task is accepted. Any other state draws
  nothing.
- **Recasting a fixed role.** `backlot recast --person producer|director|writer
  --runner <r> --model <m> [--effort <e>] [--permission-profile
  standard|worktree] [--reason <r>]` records `role.recast` in the studio's
  ledger. The reduced state uses the new identity for that role from then on;
  every earlier event keeps the identity it recorded, so deliveries and reviews
  stay attributed to whoever made them. The stage closes the old terminal with
  `role.closed` (reason `recast` — never a loss) and starts a fresh conversation
  under the same `bl-<studio>-<role>` name, never resuming the old runner's
  session; queued messages stay queued for the new terminal, and its first
  prompt says it takes the role over and must read `backlot status` and
  `backlot records` first. The command reuses `studio new --dry-run`'s runner,
  effort and cline-model rules and the studio's `[limits]` seat check, and is
  the user's alone. Hollywood gains a `recast` card (`backlot hollywood propose
  recast …`): its base is the role's current identity, its text reads `recast
  director → cmd/deepseek-v4.1-flash`, and its execution records `role.recast`
  `by: user` with the `c<n>` marker so a restart repeats nothing.

### Fixed

- **Archived-studios footer overlap, Opening Page prefix chords, and narrow stopped-stage hints.**
  The archived-studios restore list now places its `↑↓ move  ⏎ restore as…` hint on its own row
  above the `[ ↵ choose ]` / `[ esc cancel ]` buttons so they no longer overlap; the Opening Page
  now lets direct bindings and `Ctrl+B` prefix chords (`Ctrl+B t`, `Ctrl+B ?`, etc.) through while
  still consuming unhandled bare keys; and the stopped-stage rows in the 26-column cast and
  38-column lot dock switch to compact `· s start` phrasing when narrower so `start` is never
  truncated.
- **A prompt row promises only the keys that work.** A user prompt on a runner with no approval
  key (`opencode`, `cline`, `agy`) used to show the same `y`/`d` cue as cmd or claude, so the
  person answered and nothing went out. Those rows now read
  `answer in its terminal (Enter)` instead — their `y`/`d` record no decision, the Call Sheet's
  chips, footer and the role menu offer no Allow/Deny, and `p` (peek) and Enter (focus) stay. The
  needs-you list entry is kept. Driven by `can_answer_prompt` from the previous commit.
- **A permission prompt is found and named whatever the terminal's width.** A
  `bl-hollywood` resized to 21 columns, with cmd's picker on screen and its
  structured status `kind: permission` / `state: blocked`, published `work:
  blocked` but no prompt row: `hollywood_prompt` still gated on the
  width-sensitive screen classifier, and its detail took only the line under the
  sentence — a middle fragment of a wrapped command. The prompt now stands
  whenever the combined signal says blocked (structured status, hook signal or
  screen), the detail is unwrapped from the title or `needs to` sentence down to
  `Press [ctrl+e]` or the picker (cmd's whole command, the other runners'
  question), and a prompt no channel names publishes `permission prompt`. Picker
  detection joins the screen's lines first — cmd's wrapped `❯ 1. Yes` /
  `2. Yes, don't ask` / `3. No` is enough — for roles and Hollywood alike; cmd's
  and cline's prompt blocks also open at their sentence/banner when a narrow
  screen has scrolled the `─` rule away.
- **An agy role's session is found again on agy 1.5.14.** The conversation db no longer always
  embeds the workspace's `file://` URI, so the byte scan found nothing and the role raised
  `session not found` after 60 s. The stage now reads agy's own
  `~/.gemini/antigravity-cli/cache/last_conversations.json` first — a cwd → last-conversation map —
  and takes the id it names for the role's cwd when that `.db` exists, is new enough and unclaimed.
  A missing, unreadable or malformed map falls back to the old byte scan, so older agy is unchanged.
- **A restarted stage looks for a role's session again, so installing the fix repairs a stalled
  role.** The ten-minute discovery stop is measured from the later of the role's start and the stage
  process's own start. A `role.started` replays unchanged across a restart, so a role already past
  the stop previously kept its open `session not found` forever; now a restarted stage re-scans it,
  records `role.session` and clears the attention on its own.
- **A cmd role's extra directory really stops the outside-workspace prompt.**
  `--add-dir` only feeds the model's `<workspace>` context; the tool permission check
  builds its runtime without that list and reads
  `permissions.additionalDirectories` from the project settings file instead. The
  stage now writes the same directory there — the studio state directory for a role,
  `<base>/studios` for Hollywood — beside `allow` and `deny`, splicing into an
  existing file and keeping every other key. So a stage-sent message read no longer
  stops at `Outside the workspace — …`. The `--add-dir` flag stays: it still gives
  the model the directory as context.
- **Hollywood never resumes with another runner's session, and records its own
  cmd session.** A `role.state closed`, or a `started` that carries no session, now
  clears the recorded session id — after a runner change to `cmd`, the old Devin id
  no longer survived to `cmd -r <devin id>`, which answers `No session named …`. A
  resume also refuses any id whose recorded runner differs from the one being
  launched. Hollywood's post-launch discovery already ran for studio roles; with the
  stale id gone it now records `cmd`'s session from
  `~/.commandcode/projects/<slug(hollywood cwd)>/` on a `role.state started`, so a
  lost Hollywood resumes its own conversation, and falls back to the existing rule
  when discovery finds nothing.
- **cmd roles and Hollywood read their state files without a prompt.** A `cmd`
  launch now passes `--add-dir`: the studio's state directory for a studio role,
  and `<base>/studios` for Hollywood, the sibling of its `<base>/hollywood` cwd.
  A role's cwd is its worktree and Hollywood's is `hollywood/`, so the message
  files the stage sends (`<studio dir>/messages/<n>-….md`) lay outside cmd's
  workspace and every read stopped at `Outside the workspace — …` — while the
  prompt is open the stage's guarded send never delivers. The flag is per run and
  rides fresh and resumed launches alike.
- **A user config's `[runner_env]` now reaches every role's launch.** `hollywood
  setup` writes the runner's `PATH` into `~/.config/backlot/config.toml`, but the
  stage read `[runner_env]` only from the per-core `config.toml`, so the next
  Hollywood launch failed with `cannot start "cmd": No such file or directory`.
  The stage now reads the user config's `[runner_env.<runner>]` as a lower layer
  than the per-core one — `agent.env` and `[env_file]` first, then the user
  config, then the per-core table, which wins on any key it sets. Only
  `[runner_env]` is read from the user config; its `[env_file]` is ignored, and a
  malformed user file refuses with the position only, like the per-core one.

### Changed

- **Finishing an empty studio needs `--force`.** `studio finish` on a studio with no requirement and
  no tasks refuses (`nothing was built yet (no requirement, no tasks) — add --force to finish
  anyway`).
- **Finishing clears stale questions.** A finished studio shows no open `tell user`: the finish is
  the answering event, so the producer's old `What would you like us to build?` no longer lingers.
- **`close` is plain, and fixed seats stay open.** `close` on the producer, director or writer now
  refuses with `the <role> is a fixed seat — use restart, or finish the studio`, and on an actor
  that is not held with `the <role> has nothing to close — it closes with its task`. No more
  `role '...' is not held` jargon.
- **Stopping the stage moved to the machine's menu.** `Stop stage…` is gone
  from every studio's context menu: a stage is the scheduler of a whole
  machine, so stopping it halts every studio there while running roles and the
  ledger keep going. The machine row's menu now lists `Stop the stage…` right
  after `Restart the stage` while the stage runs, and its confirm names the
  machine and says the stop is machine-wide. A studio keeps `Start stage` while
  the stage is down, the one stage row that unblocks it.

## 0.3.0 — 2026-11-20

This release gives every machine its own `backlot` version, a machine menu that
installs and upgrades it, and a stage that survives failure — bounded git
calls, studio `prune`, producer alerts, rate-limit and connection-error
recovery, `stage watch --restart` and `stage service`. The ledger moves to
schema v1 with `studio migrate`, D17a hardening and factual usage and cost, and
casting gains a factual `cast --suggest` advisor and an experimental mode.

### Added

- **Factual casting advisor (L3).** `backlot cast --task <id> --suggest [--json]`
  lays the studio's `[[candidates]]` beside their cross-studio actor records for
  the task's kind: task and delivery counts, first-delivery outcomes, defects,
  stage checks, upstream changes, takeovers, final outcomes, wall time and
  usage — in declaration order, with `(not enough data: n/5)` and
  `(no history for '<kind>')` notices where the history is thin or absent.
  Matching tolerates runner-version differences and prints
  `[matched across runner versions]` when it applies. There is no score, rank
  or automatic pick (D8) — the director still casts with `--actor` and
  `--reason`. `backlot records --kind <task_kind>` filters the record view by
  task kind.
- **Experimental production mode (R14).** `mode = "experimental"` is a
  first-class studio mode: `backlot studio new --mode standard|experimental`
  overrides the cast file's `mode` (a bad value is a usage error), the
  `--interactive` wizard asks for the mode and the New Studio dialog's step 2
  toggles it. `backlot status` prints `[experimental]` on the studio header and
  the dock/sheet carry an `exp` badge — standard studios and `--plain` are
  unchanged. Each role's prompt gains a research-level appendix only in this
  mode (hypotheses, competing variants, honest negative findings), and an
  experimental studio may create a task with no recorded requirement for
  direct-to-task exploration.
- **Rate limits are recognized on reporting cores.** A terminal whose status
  reads `done`, `idle` or `waiting` — Devin's rate limit reports `done` — now
  has its fresh screen tail read like a silent core's, so Devin's
  `Reached free model rate limit … reset in 54 minutes` and Claude Code's
  measured screens raise `rate limited` instead of waiting for `stall_limit_s`.
  A `working` terminal is never matched, and an `error` keeps its existing
  message-then-screen order. `hit your limit`, `session limit`, `weekly limit`
  and `monthly limit` are recognized words.
- **A weekly or monthly quota escalates at once.** Limit text naming a weekly
  or monthly limit raises `rate limited` with `needs the user` immediately —
  no three futile `continue` retries — while a `rate_pace` sentinel keeps a
  cleared attention from re-raising on the same banner, and the banner going
  clears `resumed`.
- **Connection errors are recognized and retried.** Devin stops on
  `Connection error` until sent a short message; the stage now raises its own
  `connection error` attention, sends `continue` after 15 seconds, and after
  three failed attempts escalates `needs the user`. A rate limit on the same
  sample wins, and a reporting core's bare `agent error` is suppressed while
  the connection error holds. The banner disappearing clears `resumed`.
- `backlot stage watch --restart` supervises the stage: a dead
  `backlot-stage` is started again through the same idempotent path as
  `stage start`, under a bounded budget (five starts in ten minutes, an
  exponential 2→4→8→16→30 s backoff, reset after a stable minute), and pauses
  once per episode when the budget is spent. A dead core is reconnected with a
  2→5→15→30 s backoff, announcing the edge once, and the stage is checked
  again immediately. `stage watch` without `--restart` is unchanged.
- `backlot stage service [--print | --write] [--session <s>]` renders a
  systemd `--user` unit (`backlot-stage.service`, or
  `backlot-stage-<session>.service`) that runs `stage watch --restart`;
  `--write` installs it under `${XDG_CONFIG_HOME:-$HOME/.config}/systemd/user/`
  and never invokes `systemctl`. Working roles are refused, `--print --write`
  is a usage error, and `--remote` is refused because a user unit runs on the
  local host.
- **Every machine shows its `backlot` version.** The stage publishes
  `backlot_version` in its `backlot/1` document, so a machine whose stage runs
  answers for itself; machines without a stage (or an older stage doc) are
  probed once — `command -v`, then a login-shell lookup, then `~/.local/bin` —
  and cached for six hours, on connect, on an offline → online return, and
  after an install. In studio mode the expanded sidebar's machine rows carry a
  second line: `hc <v>` (the endpoint's core version, dropped first at narrow
  widths) plus `backlot <v>`, an accent `↑<latest>` when the machine is behind
  the known release, `no backlot`, or a dim pending/error word. `--plain`
  keeps Herdr's one-line rows untouched.
- **The machine row has its own menu.** Right-click (or the launcher) on a
  machine offers only what is valid there: `Install backlot` when it is
  missing, `Upgrade backlot to v<latest>` when it is behind, `Start`/`Restart
  the stage` on the stage's state, `New studio here` when a `backlot` can
  answer, and the endpoint ops (`Connect`/`Rename`/`Remove machine`, `Add
  machine` on the local row). An unreachable machine dims the command rows —
  the endpoint ops stay live as the repair path — and `Ctrl+B ?` lists the
  same actions in a `machines` group when any apply.
- **Install and upgrade run on the machine.** The menu's install/upgrade first
  probes the target's `backlot` path and environment read-only; a binary
  managed by mise, Homebrew, Nix, cargo or a dev `target/` build is refused
  with the channel's own command — never an `install.sh` over a package
  manager's file. Otherwise a popup terminal opens on that machine
  (`terminal.run on: <machine>` remotely) running the official installer with
  `BACKLOT_SKIP_CORE=1` (hc is never touched), a semver-validated
  `BACKLOT_VERSION=v<latest>` pin and a shell-quoted `BACKLOT_INSTALL_DIR`.
  Closing the popup invalidates the located-path and version caches and
  re-probes; an upgraded machine whose stage was running is restarted —
  `stage stop` then `stage start` through the action tracker, and the ledger
  replays so no studio state is lost. The `stage not running` dock rows are
  now clickable too: Enter or a click starts that machine's stage.
- `backlot studio prune [<studio> | --all-finished] [--dry-run] [--force]` reaps a finished
  studio's worktrees and `backlot/<studio>/*` branches. A live studio is refused (exit 2);
  without `--force` a dirty worktree is kept and branches go only through `git branch -d`, so
  unmerged work is never lost. `--dry-run` reads git and prints the plan. The ledger,
  `reports/`, `messages/` and `attention/` are never removed, so `backlot records` and
  `backlot status` keep the full history.
- A producer that stalls or cannot resume now tells a live director once that it cannot restart
  the producer and only the user or Hollywood can intervene. `backlot status` and
  `status --all` lead the studio block with a `producer alert:` line, and Hollywood's notice
  names the producer outright (`producer stalled` / `producer cannot resume — needs attention`).
- **Ledger schema versioning and migration.** Every studio and Hollywood event now carries
  `v: 1`; unversioned lines still load as `v: 1`, and a `v` newer than the binary refuses
  with a clear "upgrade backlot" error. `backlot_model::migrate` is a pure step-by-step
  pipeline ready for future versions, and `backlot studio migrate
  [<studio> | --all] [--dry-run]` rewrites a log in place — backup kept, atomic rename,
  replay-verified — while a current log is a no-op.
- **Runner-reported usage and cost.** `backlot deliver` accepts `--tokens-in`,
  `--tokens-out`, `--cache-read-tokens` and `--cost-usd` (parsed to exact integer
  micro-dollars), recorded as an optional `usage` object on `task.delivered`. The sums are
  factual only — `StudioState`, `ActorRecord` and the `backlot/1` summary carry totals, and
  `backlot status`, `backlot records`, the studio dock (`∑ 12.4k/3.1k tok · $0.08`) and the
  studio sheet show them only while a counter is non-zero. No score, per D8; `--plain` is
  unchanged.

### Changed

- Synced with hc-tui v0.1.16: the agents list follows the focused pane, so a
  tab, workspace or core-driven focus change scrolls the focused agent row
  into view (hc-tui #53).
- Every stage `git` call is bounded at 60 seconds. A hung git is spawned in its own process
  group, gets SIGTERM then SIGKILL (one second later), is reaped, and surfaces
  `git timed out after <n>s` instead of blocking the stage tick for every studio.

### Security

- **D17a boundary hardening.** A permission prompt whose block reaches for `BACKLOT_`
  reads/`unset`/`export`/`declare`, `env -u`/`--unset`, `/proc/*/environ` reads or
  `env`/`printenv` dumps coupled with secret/`.env` paths is now flagged `needs_user`
  alongside the existing `env -i`/`--ignore-environment` checks. `report show --file`
  rejects NUL bytes and keeps its canonicalized-target containment; studio and card
  identifiers are validated before any path or argv is built (including `..` draft
  escapes and leading `-`), and the client's action runner refuses argv or machine
  values carrying control bytes.

## 0.2.4 — 2026-11-19

### Added

- **The studio dock is the unified inspection surface.** `Ctrl+B y` still opens
  it, but the panel is now the one place a studio's people, tasks and
  deliveries are visible and inspectable. Each studio block carries a health
  glyph (`●` live, `◌` stage stopped, `×` offline or ledger error, `✓`
  finished), `r<version> · <done>/<total>` progress and a `!<waiting>` count;
  the owed-step line names the role the step is owed to (`→ deliver t1 ·
  actor-a`) instead of the raw ledger key. People rows show the `USER` badge
  and the read-only `dir` tag on director-owned prompts, and a `[y/d/p]` cue
  line on user-owned ones. Tasks with no live actor get their own rows —
  uncast, dependency-gated, paused, at rework cap or blocked, and accepted —
  and accepted tasks fold under `Space` when there are six or more. `Space` on
  a person or task opens a three-line drawer; on a studio header it folds the
  block. The dock's width is fixed (38 columns at 160+, 36 at 100+, 28 under),
  it sits beside the panes while they keep 44 columns and overlays the pane
  surface below that; `Esc` unfocuses it beside the panes (borders mute,
  `Ctrl+B y` refocuses, `q`/`Ctrl+B Y`/`×` closes) and steps back over an
  overlay — to the 16-column strip when it fits, else closed — with the footer
  reading `esc back`. The collapsed strip shows health, a short studio/machine
  label and the active or blocked roles.
- **The `v` Task & Delivery Sheet.** `v` on a dock row opens a centered modal —
  inside the pane surface, never over a beside dock — with three tabs: Overview
  & Casts (acceptance, dependency chain, every cast), Deliveries & Reviews
  (commit range, actor-reported and stage-run check exit codes and durations,
  verdicts and findings) and Actor Records. The summary paints at once while
  `status --json` and `records --json` load asynchronously on the studio's own
  machine; `o` streams a delivery report or check log, locally or through
  `backlot report show` remotely.
- `backlot report show --studio <s> --file <relpath>` streams a studio-relative
  file under `reports/`, `checks/`, `attention/` or `requirements/` to stdout
  — `..`, absolute paths and symlinks that leave the studio are rejected, and
  the stream is capped at 1 MiB.
- **The dock's keys.** `Enter` focuses a live terminal (on a closed person it
  reopens a resumable or fixed seat through the action tracker; `o` does the
  same for a resumable one), `p` peeks,
  `y`/`d` answer a user-owned prompt, `+`/`-` set a task's priority, `r`/`f`
  run the stage-start and studio-finish actions, `c` collapses, `q` closes,
  `?` opens help, and `m` opens the row's context menu.
- **Studio context menus everywhere.** `m` on a dock or Call Sheet row,
  right-click on a dock row, a Call Sheet row, a sidebar cast row or the
  studio header, and `Ctrl+B m` from a focused pane (a role pane's role menu,
  a shell pane's studio menu) all open the same menu for that target —
  focus/peek/sheet navigation, prompt answers (the D9 director override goes
  through a confirm), task verbs, tells, stage and finish. Items a press
  cannot run are dimmed with their reason or hidden, the menu clamps inside
  the screen (`min(34, cols - 4)`), and each item's shown key activates it.
- **The native studio dialogs.** Menu `…` items open a one-line reason field
  (pause/resume/abandon/takeover; card cancel and studio finish keep the
  reason optional), a confirm card (close a role, stop the stage, clear
  stale attention, answer a director-routed prompt yourself), the four-row
  Tell editor (`tell <role> --text`), and the Revise editor prefilled from
  `requirements/v<n>.md` (`Ctrl+E` rewrites it in `$EDITOR` locally; a remote
  studio prefills through `backlot report show` and submits `req revise
  --stdin` over `terminal.run`). `Esc` cancels, `Enter`/`y` submits, and
  dialogs center in the modal band beside the dock.
- **One action lifecycle.** Every `backlot` call the TUI makes — dock keys,
  Call Sheet rows, menus and dialogs — runs through the ActionTracker: the
  target row spins `⠋ <verb>…` while it runs (a repeat of the same command
  waits), a success toast holds 3.5 s, and a failure card holds 8 s with the
  first `stderr` line, always naming a remote machine (`host-b: paused task
  t1` / `host-b failed: …`). `C` or `copy cmd` writes the exact command —
  `ssh <machine> -- backlot …` for a remote studio — to the clipboard
  through OSC 52.
- `backlot stage stop [--session <s>] [--remote <t>]` ends the stage's
  terminal gracefully: `stage stopped`, or `stage already stopped` when no
  stage is up.
- `backlot tell <role>` and `backlot req revise` take `--text <msg>` /
  `--stdin` alongside `--file` — exactly one source each, empty text
  refused.
- The `Ctrl+B ?` help gains a `studio dock (open)` group; dialogs (the sheet,
  the New Studio dialog, help) share one geometry and center inside the pane
  surface, so no dialog ever crosses an open dock's border.

- **The native New Studio dialog.** Every studio-mode new-studio entry —
  `Ctrl+B N` over a studio, `n` on the opening page or in the Call Sheet, the
  `+ new studio` frame (a click now opens it), and the sidebar's
  `new · <machine>` with that machine preselected — opens a four-step overlay
  in the client instead of the cooked-stdin wizard popup: **1 Machine & Repo**
  (Local plus every attached machine, probed for reachability and
  `backlot --version`, `Ctrl+R` re-checks; the repository browser has a
  breadcrumb, a `/` filter and `→`/`←` descend/ascend), **2 ID & Title** (live
  validation and a duplicate check against that machine's studios), **3 Cast**
  (per-role runner/model/effort cycled among what the target machine detected,
  actor candidates added with `a` and removed with `x`, `Ctrl+E` edits the
  generated cast.toml in `$EDITOR`), **4 Check & Start** (`studio new
  --dry-run`, then `Enter` creates on the target machine and optionally starts
  its stage, after which the new producer's terminal takes focus). `Esc` steps
  back through the steps and then closes; `Ctrl+C` cancels from any step. A
  studio can now be created on any attached machine: every question is answered
  by running `backlot … --json` there over `terminal.run`, remote repository
  paths are never checked against the local disk, and the generated cast
  travels on stdin (`studio new --cast -`) so no file is left behind. In a
  terminal, `backlot studio new --interactive` still asks the same questions.
- `backlot repos --json [--from <dir>]` lists a directory's entries for the
  dialog — each classified `git_ready`, `git_empty` or `dir`, with the studio
  that uses it — defaulting to `$HOME`.
- `backlot runners --json` prints the runner detection as JSON, with the
  models and efforts backlot knows per runner.
- **`Ctrl+B ?` is the studio action launcher.** In studio mode the help
  overlay keeps Herdr's keybind reference and gains runnable rows: a
  `waiting on you` group first (your prompts, stopped stages, Hollywood
  cards and studios ready to finish), then `studio & task actions` and
  `hollywood`, then Herdr's own sections. `Enter` on a runnable row runs
  the action through the same context-menu path a right-click uses —
  opening a compact target picker or the S3 reason dialog when a target is
  needed — and `Enter` on a reference row focuses its surface. `/` filters
  (`/pause` finds `Pause task…`), the glyph legend stays pinned, and the
  sidebar's `menu` button opens the overlay. With no studios and no
  `[hollywood]` — or under `--plain` — the list stays Herdr's own,
  cell-identical.
- **Every studio surface answers the mouse.** Call Sheet rows select and
  act like `Enter`, chips run their action, the `tell hollywood` line
  opens for editing on click, and the bar's scrollbar drags while the
  wheel scrolls rows vertically or the frame strip horizontally;
  right-click on the title or a row opens its S3 menu. Clicking outside
  the expanded bar collapses it to the lot pill and hands the keys to the
  pane underneath. The lot pill's left-click toggles the bar and its
  right-click opens the studio header's menu; studio header clicks focus
  the studio; sidebar cast rows focus their person and the `others` row
  expands in place. In the dock, left-click selects and activates,
  right-click claims the dock's keys and opens the row menu, the scrollbar
  clicks, drags and wheels, and dragging the left border resizes the dock
  between 28 and 48 columns. Every path that opens an overlay or selects a
  row claims the keys for that surface; every path that focuses a pane
  gives them back.

### Changed

- **A tab switch keeps its screen, and a cold open has more history.** A cold
  attach asks the core for 5000 lines instead of 200, every pane's VT takes the
  10 MB scrollback budget, and switching back to a tab continues the VT it left
  instead of replaying history — about one screen on the wire instead of 5000
  lines. `[advanced] attach_history_lines` (default 5000, clamped at 50000)
  and `[advanced] scrollback_limit_bytes` (default 10000000) tune both. A
  resumed attach, a changed terminal, an evicted observer or a screen that
  never arrives fall back to a cold open with full history. The studio's role
  terminals get the same cold open and the same warm handover.
- **The expanded Hollywood bar is the Call Sheet.** One list of everything
  waiting on the user across every machine — user-owned prompts first, then
  stopped stages and offline machines with live studios, then open cards, then
  studios ready to finish — each row answered from the bar with keys: `y`/`d`/`p`
  on a prompt, `r` on a stage, `a`/`x`/`s` on a card, `f` with an inline `y/n`
  confirm on a ready studio, `i` for `tell hollywood`, `?` for help. Remote
  prompt, stage and finish actions run on their own machine; cards and `tell`
  run on the local core. Clicks select; activation stays on the keys.
- Frames carry their own machine's stage and connectivity, and a
  machine-qualified label (`beta-tools · host-b`) that truncates the machine
  before the id. Under height pressure the frames fold into a one-row chip
  strip, details and the headline go before the waiting rows do, and the list
  scrolls with a `i/N` marker.
- The summary document gains `attention_seq`/`attention_detail` on roles and
  `tasks_done`/`tasks_total` on studios — additive, `backlot/1` unchanged.
- `x` in the expanded bar is the card cancel; `Esc` or the toggle collapses.

### Fixed

- A program that redraws with synchronized output (`CSI ? 2026 h` … `l`, as
  Antigravity CLI and Claude Code do) no longer flickers: a frame's bytes are
  held until it closes, bounded at one second and 256 KiB, so the pane never
  paints a half-written line or a cursor in the middle of the old text.

## 0.2.3 — 2026-11-18

### Added

- The in-client update, on backlot's own release: `prefix+u` (or a click on
  the `↑v…` hint) opens the update card with the release notes of every newer
  release, `↵ update` runs backlot's `install.sh` for the latest release,
  verifies `backlot --version`, and `↵ restart` re-executes the installed
  binary in place. A package-managed or dev build is refused with its own
  command, and `BACKLOT_INSTALL_DIR` names another directory.
- A running-version badge in the sidebar header (and in the mobile header),
  with an accent `↑v…` hint for a newer release and a green `↻v…` once an
  install waits for its restart. `BACKLOT_NO_UPDATE_CHECK=1` still opts out.
- `◌ waiting` for an agent whose own turn is quiet while work it started is
  still running.
- The sidebar and panel chrome the user changed by hand — width, the
  spaces/agents split, collapse state, the agent-panel sort — is remembered
  per endpoint across restarts.
- An opt-in diagnostics snapshot.

### Changed

- **Studio workspaces drop the tab bar.** A one-row studio header
  (`studio › role t1 ◐ ↑`) takes its place. `Alt+↑/↓` switch studios, and `Alt+←/→` step
  through the cast. Elsewhere Herdr's tab keys apply. Other workspaces keep Herdr's tab bar.
- **The lot pill** (`✦ N · !U you · C card`) replaces the floating collapsed
  strip. It turns red when something waits on you. The Hollywood bar starts
  collapsed (`[hollywood] start`).
- **One glyph and colour vocabulary** across the panel, the cast sidebar and
  Hollywood. `idle` is a grey `○`, and only `working` animates.
- **Panel selection** starts on the first actionable row. `j`/`k` skip
  dividers. The highlight stays inside the block's borders and hides while the
  panel is unfocused. Hints never cut a word.
- **`Ctrl+B ?` in studio mode** leads with backlot's sections. The glyph legend
  is pinned above the footer.
- **Settings:** a `studio` tab with call-sheet start, direct `Alt+Y`, dock
  width and the overseer. An unset overseer shows the setup command.
- The agent panel's default sort is `priority` (waiting first), and a sort
  the user picked is remembered.
- Official `hc` 0.1.21 is the tested core; older 0.1.x cores keep working,
  with each missing capability switched off. The pinned test core and the
  schema fixture move with it, and the fork's client contract is linked from
  the README and [core compatibility](docs/compatibility.md).
- The studio keeps naming the `backlot` document in exactly one place
  (`WatchParams.documents`) and names no documents in `events.subscribe`:
  on 0.1.21 and later cores that would silence terminal events.
- Image paste over SSH sends acknowledged pieces when the core advertises
  `put_chunks`, and one `terminal.put` otherwise.

### Fixed

- The studio cast stays in the sidebar when the core has several machines.
- A `cancelled` `terminal.send` is a refusal: the stage and the client both
  say nothing was typed, the message stays queued, and a timed-out send never
  reads as "not typed" (the core may still type it).
- Pane output follows the tail when a key is pressed (not row 0), attaches
  release their call registrations when they close early, and a narrow mobile
  header keeps the studio's name before the update hint takes its cells.
- The parity gate compares again: herdr's own agent-panel sort in the
  reference config, a one-shot popup pin, one fixture directory for both
  clients, the fixture's cursor shape, and exact-cell known differences with
  their reasons.

