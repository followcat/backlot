# Milestone progress

## M0: Bootstrap (2026-11-04)

- Implemented in `45cc5eb`; supervisor review and the private-data scan passed.
- Added a Rust 2024 workspace with the `hc-tui` binary and `hc-proto` library.
- The CLI provides help, version, and an honest bootstrap message. It opens no
  core connection; the interactive UI is not implemented yet.
- Added CI for formatting, all-target Clippy with denied warnings, and tests.
- Validation: the three checks pass locally; six tests cover the supported
  protocol generation and CLI behavior using temporary XDG directories.
- Bootstrap code passed GitHub CI before its message/author metadata rewrites.
- Open: protocol transport and scratch-core integration arrive in M1. Startup
  performance and live UI behavior have not been measured.

## M1: Protocol client (complete)

- Task 1 (types and wire codecs): hc-proto now carries the typed params and
  results for the methods hc-tui uses, strict JSON-RPC 2.0 envelopes with
  structured `data.kind`/`hint` errors, a hello capability helper, and the
  bounded line and framed codecs over a shared read-ahead buffer. Conformance
  is tested against a pinned, sanitized copy of the OpenRPC schema
  (herdr-core `ec8adcdf`). The wire/envelope layer landed in `39b4247` and
  the typed layer in `bcb9e07`. Validation: the three checks pass locally
  with 59 tests, and supervisor review passed on the combined work.
- Task 2 (endpoint discovery and transport): hc-proto resolves a core's
  endpoint from injected environment values — session rules, `HC_SOCKET`,
  the `socket` config key, XDG runtime/state fallbacks and the socket-path
  length bound — and opens an owned connection: a Unix socket locally or one
  ssh child remotely (`-T`, batch auth, bounded stderr tail, reaped on
  close/drop). Discovery itself never connects or creates state. RPC
  dispatch, attach streams and subscription recovery are still open tasks.
  Validation: supervisor formatting, Clippy and 82 tests passed; ten repeated
  parallel transport runs passed after fixing the test-helper exec race.
- Task 3 (concurrent RPC client): hc-proto's `Client` negotiates `hello`
  over the line codec and switches to framed mode without losing bytes the
  peer coalesced into the same read. Channel-0 calls run concurrently over
  one writer queue, correlated by id — a held call cannot stall others, and
  out-of-order replies still find their waiter. One deadline covers both
  enqueue and reply; a cancelled call removes its pending entry itself
  (no peer activity or later call needed), registration is atomic with the
  final fail-all, and the bounds are 256 in flight and 64 queued frames.
  Either half's death runs one shutdown: pending calls fail, the sibling
  task is aborted and the backend child gets a bounded graceful close;
  malformed control-channel JSON-RPC payloads and null-id replies fail as `Protocol`
  errors while valid notifications and late replies are ignored.
  `hello` capabilities gate each typed call; `rpc.discover` is bounded at
  5s, refines method availability, never blocks calls, and reports honest
  notes for every unusable outcome. `close` takes ownership of the backend
  before its first await so a cancelled close still kills and reaps an ssh
  child; `hello` failures drop the codec halves first (the child sees
  stdin EOF) and keep its exit status and stderr tail without masking a
  genuine protocol answer. Attach streams and subscription recovery
  remain open. Validation: the three checks pass locally with 123 tests
  over scripted fake cores, fault-injected halves, fake-ssh children and
  a scratch Unix socket. Independent supervisor review and all three gates
  passed; ten parallel SSH-client suite repetitions also passed (50 tests).
  CI then exposed a fake-SSH readiness race: the child now publishes its PID
  before its hello reply, so immediate cancellation can still verify reaping.
  Shared script initialization also prevents concurrent forks from inheriting
  open script writers, which caused an intermittent `ETXTBSY` spawn failure.
  GitHub CI passed for the fixture fix in `4756efd`.
- Task 4 (attach streams): `Attach` gives each terminal its own framed
  connection, preserving reply/paint read-ahead and ordered DATA/RESIZE.
  It exposes authoritative terminal facts, observer restrictions, focus/resize/
  display controls and source-tagged automatic replies with negotiated fallback.
  Byte/count bounds and explicit consumed-byte credit keep a stalled stream
  independent of RPC; CLOSE wakes blocked producers and receivers. Opening,
  detach and drop own cancellation-safe cleanup without killing the terminal;
  detach shares one deadline across CLOSE enqueue and acknowledgement.
  Validation: independent formatting, Clippy and 157 tests passed, plus ten
  repetitions of all five attach suites (330 tests). Tests include a genuinely
  stalled peer, event floods and SSH child reaping after stage-synchronized
  cancellation. Subscription recovery and real-core integration remain open.
  GitHub CI passed for `cc8bdfa`.
- Task 5 (subscription and inventory): a dedicated plain subscription keeps
  snapshot/notification read-ahead, validates generation/methods and rejects
  pre-reply or malformed traffic. Terminal errors close the owned transport;
  cancelling a healthy partial read preserves its buffered bytes.
  The pure inventory reducer applies known events atomically, retains unknown
  kinds, detects unfiltered sequence gaps, replaces same-sequence missed
  snapshots and preserves offline inventory. Document-refetch effects are
  exposed without interpreting documents or inferring status.
  Validation: independent formatting, Clippy and 201 tests passed, plus twenty
  four-thread SSH suite repetitions (80 tests) with separate PID/reaping proofs.
  Document/machine refresh, reconnect and real-core integration remain open.
  GitHub CI passed for `1db13c4`.
- Task 6 (recovery watcher): `watch::Watcher` composes `Subscription` and
  `Inventory` into a published `View` (inventory, machines, documents, link
  states, notes) through an actor that owns both connections. Reconnect uses
  per-facility backoff; a lost subscription keeps the offline inventory and a
  dead RPC invalidates its epoch while the subscription epoch survives.
  Document reads stay bounded (`doc_concurrency` globally, one in flight per
  source) and staleness is tagged by rpc epoch plus persistent per-document
  generation, so results from dead conns, superseded requests and
  removed-then-returned machine aliases can never install. Feature flags
  re-probe once per fresh hello and `events.subscribe` falls back to
  `terminal.list` statically. `changed_since` cursors lose no publish;
  `close`/`stop`/`Drop` abort openers, fetches and owned children —
  including a `close` future cancelled mid-flight.
  Validation: independent formatting, Clippy and 221 tests passed; 20 watcher tests cover
  snapshot, events, heartbeat, missed, ordering, staleness, fairness,
  capability recovery, held/cancelled opens and static mode over duplex
  fakes, plus twenty four-thread repetitions (400 tests). Real-core integration
  remains the next task.
  GitHub CI passed for `8c340a0`.
- Task 7 (real-core integration): `tests/integration.rs` is an opt-in
  suite gated on `HC_TUI_TEST_CORE` — missing binary fails the explicit
  run, ordinary `cargo test` never touches a real `hc`. Each test owns a
  `ScratchCore`: random `hctui-test-*` session, cleared inherited `HC_*`,
  temporary XDG/HOME roots, pre-seeded `host-a`, foreground `serve`, and
  `server stop` verified by `stopped:true` — unclean exits are recovered
  by an adopting replacement serve before bounded reaping; the root is
  kept on verified teardown failure. Coverage: hello/discovery,
  subscribe-before-run, run/read/attach paint+input/kill events, watcher
  reconnect with a new epoch and a doc refetch proven by an on-disk
  v1→v2 swap, plus a pid-disappears teardown regression for detached
  holders. `fixture.finish()` lets a successful test fail explicitly on
  teardown failure while panic paths preserve evidence without panicking.
  `tools/build-test-core.sh` fetches the pinned core ec8adcdf, Ghostty
  1e5cdb86 and Zig 0.16.0 (sha256-verified) into `target/` with pin-stamped
  caches, `--locked` and bounded curl retries; `docs/integration.md`
  documents pins and isolation; CI runs the suite.
  Validation: independent formatting, Clippy and 221 default tests passed
  (four real tests explicitly ignored). Real integration passed against the
  pinned build and a separately preserved compatibility build, including
  forty repeated checks; the clean helper build passed all four real tests.
  Explicit missing-binary runs failed as required.
  GitHub checks and real-core integration CI passed for `a77fe51`; M1 complete.

## M2: Read-only terminal UI (exact parity not accepted)

- Replaced the earlier provisional sidebar with directly attributed ports
  of herdr 0.9.5-followcat `d00953e0` thin-client chrome, sidebar, config,
  navigation, key leases and input parsing. The old word/source badge
  presentation is removed. Herdr configuration is read without migration.
- Added the event-driven UI, one active attached VT, searchable help,
  workspace switching, sidebar mouse interaction, history scrolling,
  host capabilities/cursor policy, titles, tab status, resize and explicit
  size control. Source status presentation is adapted from authoritative
  core records, with local acknowledgement/chronology caches.
- Input waits for complete initial mode restoration, retains FIFO order
  with viewer controls, and stays bound to terminal incarnation and attach
  generation. Healthy switched routes retain real key releases; graceful
  detach drains delivered keys, then focus, DATA fence and CLOSE. Ambiguous
  disconnected batches are rejected visibly as a whole.
- Added the owned-session native PTY parity harness and immutable neutral
  reference input captures. Full default-config coverage has 32 checkpoints
  at each of 80x24, 120x40 and 200x50; five additional configuration profiles
  cover drawn cursors, literal tab status, conflicting direct/prefix keys,
  datetime sanitization and custom window titles. Input coverage includes
  390 actual child-byte cases plus post-detach releases/focus.
- Independent Claude and separate read-only Codex reviews identified
  correctness, parity and fixture-safety issues. Fixes include fragmented
  paint/input gating, incarnation checks, control/input ordering, global
  forwarded-key replacement, native functional-key classification and
  disabling sound/automatic commands in scratch fixtures.
- Implementation: `f96a7f1`; owned-session harness: `b68ff0a`.
  Validation: formatting, all-target Clippy with denied warnings, 477
  default Rust tests (four real-core tests opt-in), four explicit real-core
  integration tests and six harness safety checks pass. Native parity is
  96/96 default-config checkpoints and 160/160 configuration checkpoints,
  with zero cell/mode differences; only the precise default-title hostname
  adaptation is recorded. All 390 paired input cases and 14 detach-tail
  comparisons pass. Raw mode and host modes restore; owned fixtures clean up.
- Real local and loopback SSH checks pass shell input, switching, core
  crash/restart, visible offline state, retained paint, recovered input and
  detach without killing programs. Ten warm debug-client starts reached
  useful paint at p50 91 ms / p95 143 ms, all below 200 ms. Default idle
  samples used 0–0.3% client CPU over ten seconds; these are local fixtures,
  not a promise about remote network latency or a release build.
- Joint review did not accept indistinguishable parity: ordinary Git rows,
  routine switch-time first-paint appearance and narrow native evidence
  required follow-up. See [parity](parity.md) for exact tested-core
  limitations and the [runtime](runtime.md) and
  [harness](../scripts/parity/README.md) for usage.
  M3 layout/shared documents/full bindings and M4/M5 actions/copy features
  remain outside this implementation. No release is published.


## M2.1: Joint-review remediation

- `ee8346e` tightens intended-state readiness and native host mode/cursor
  restoration, and restricts unordered detach releases to source-derived,
  position-preserving fixture segments. Ordinary input expectations are
  unchanged. Strict120x40 and390paired input cases pass.
- `4d1b124` adds source mobile menu presentation and suspended-Help lifecycle,
  including inventory-removal focus recovery.
  Native results:52/52 checks at65/64/40 columns,96/96 at the three desktop
  sizes, plus32/32 in a host ModifyOtherKeys1 profile. Requested statuses,
  highlight and mode transitions match; native host/cursor restoration passes.
- The local-only Git fixture verifies real ↑2 ↓1 divergence, linked common
  Git directory and an unlabelled src terminal. Source-visible branch/counts
  and repo-root label are required before capture. The pinned core scenario
  records66 raw differences, passed:false, exit2 and pending-core-fact.
  No expected differences are promoted to passes or hidden by region masks.
  M2.2 adopts C3 only after Claude approval; this does not accept M2 as exact.
- Final native candidate passes180checkpoints,390paired input cases and14
  position-preserving detach-tail checks. Each slice has an independent
  read-only review; addressed findings cover stale resize/selection/readiness,
  suspended Help and stale focus after inventory removal. Full gates pass479
  default Rust tests (four integration tests opt-in), two native decoder tests
  and nine harness checks. Expected input bytes were not regenerated.
- M2.1 stops for review after normal push/green CI. M2.2 requires Claude's C3
  approval; M3 remains gated. No core extension was accessed or adopted here.


## M3a: Shared structure and layout — accepted with M3a.1 follow-up

- Added the read-only herdr-app reader with label/cwd fallback, attributed
  pinned BSP layout, and shared schema-1 hc-tui documents. Versioned writes
  preserve opaque fields, merge explicit CAS conflicts, fence stale topology
  and core identity, and coordinate concurrent fallback-group adoption.
- Implemented layout/tab navigation, focus/swap/cycle/last pane, zoom/resize,
  tab/seam dragging, Ctrl+G lock and the matching default/configured bindings.
  M3a includes new tab, both split axes, close and rename with source dialogs.
  Creation labels support herdr-app adoption; close rechecks incarnation and
  membership before kill. Kill/write remain separate calls; ambiguous or
  partial failures are visible and mutations are not blindly replayed.
- Current-tab VTs remain attached plus 32 off-tab LRU VTs. As explicitly
  corrected by the user, geometrically visible panes hold silent size leases;
  they send no human input, focus or automatic replies. Off-tab LRU entries
  stay strictly observe-only. Retained VTs preserve live appearance through
  switching and resize paint. New targets open at their actual allocation;
  viewer size requests are not coalesced against another viewer's grid.
- Native M3a evidence: 196/196 default and 220/220 configured checkpoints at
  80x24, 120x40, 200x50, 65x24, 64x24 and 40x24, plus 36/36 with only the
  previous-tab move binding configured. All available-fact cells/styles,
  cursor and host modes match. Actual PTY grids and exact input gates pass.
  Two real clients commit both concurrent rename intentions and converge;
  fake-core tests force CAS conflicts. Idle one-shot cursor/palette return
  passes without fresh program output; unit tests also inspect OSC 12.
- Final-candidate M2 regression passes 148 six-size checkpoints plus 32 in
  the host ModifyOtherKeys1 profile. All 390 paired immutable input cases
  and 14 positioned detach tails pass. Host modes/termios restore, owned
  scratch fixtures clean up, and the original config remains unchanged.
  The default hostname adaptation remains explicit. The repository fixture
  still records 66 raw differences, exit 2 and pending-core-fact, never a pass.
- Every code slice had an independent read-only review and full gates.
  Final local validation passes 575 default Rust tests, four explicit real-core
  tests, two decoder tests and 14 harness safety tests; four integration tests
  and one manual BSP profile remain opt-in in the default suite. Commits:
  c3f2520, 05c52c1, ed04aaf, d7d0b87, 6171b7a, 74d1ac8 and 607c4c3.
  CI 42085503012 is green for the final code commit 607c4c3.
- M3a stops for joint review. Exact M2 is still not accepted: hostname,
  Git/worktree facts, quiet release and chronology await reviewed C3 facts.
  C3 will not supply first-paint appearance; first attach, reconnect and
  eviction remain limitations. Acknowledgement is client-local with the
  documented older-stream stall race. Settings/reload actions and remaining
  M3 work are not completed by this slice. Custom commands, notifications
  and other M4 actions remain deferred. No M4 work or release is started.


## M3a.1: Configuration and passive size recovery — ready for review

- M3a is accepted. Added live reload through Ctrl+B then Shift+R and the
  mobile menu: bindings, theme and allocated geometry update together.
  Malformed sections keep their current values while valid siblings apply;
  read/parse failures retain the configuration and show a source diagnostic.
  Manual sidebar width and agent sorting survive reload.
- Settings now shows the transient notice “Settings isn't available in
  hc-tui yet” from its binding and mobile menu. The overlay remains M4;
  its native differences are retained as pending-feature, never parity passes.
- Size-holder recovery now uses passive resumed attaches. An owned proxy
  cuts desktop transports while a separate phone-size viewer stays active.
  Successful concurrent live replies and fresh observer output are required:
  the phone retains size, detaching restores the passive desktop size, and
  an actual desktop allocation change can reclaim it. Fresh tab entry and
  focus remain real activity. No mismatch-driven reclaim loop is introduced.
- Native layout/reload checks pass 250/250 default and 274/274 configured
  checkpoints across 80x24, 120x40, 200x50, 65x24, 64x24 and 40x24, plus
  44/44 for a previous-move-only profile. Both reload rounds apply new prefixes,
  Help bindings, themes and width on both clients; mobile reload is exercised
  at 64/40 columns. All 17 Settings notice checks preserve their pending-feature
  classification. M2 regression passes 180/180; paired immutable input cases
  pass 390/390 and positioned detach tails pass 14/14.
- The native decoder additionally compares emitted host cursor-color overrides
  and tests OSC12/set/reset. Cached-switch captures have no such override on
  either side; retained program OSC12 remains unit-only evidence. Position,
  visibility, shape, blink, cells and indexed palette checks remain native.
- Documented hc's latest-activity policy versus herdr's direct-attach lock.
  Canonical terminal IDs are not recycled within preserved core state; names
  can be reassigned. A move after validation can still race kill. Displayed
  membership authorizes explicit close; no adoption gate was added.
- Independent reviews addressed successful-versus-attempted recovery evidence,
  manual agent sort, valid TOML formatting and DEL escaping in the scratch
  rewrite. Exact local gates pass 581 Rust tests, four explicit real-core tests,
  three decoder tests and 21 harness safety tests. Owned cleanup, host/termios
  restoration, original-config hashes and leak scans pass. Expected input bytes
  are unchanged. Commits: 53ba001 and 86378fa; their CI runs are green.
- M3a.1 stops for review. Settings overlay and other M4 actions remain deferred;
  no M2.2/C3 adoption, M4 implementation or release was started. First-attach,
  reconnect/eviction appearance and acknowledgement-fence limitations remain.


## M2.2: C3 facts — ready for review

- Adopted typed repository/activity snapshots, complete replacement events,
  atomic status/activity and explicit nullable resets. Malformed complete facts
  leave records and sequence unchanged. Origin clocks retain core/epoch/scope/
  revision; lifetime-only resets baseline acknowledgement without advancing order.
  Chronology and completion inference remain M3b.
- Verified and pinned the published `hc 0.1.17-followcat.8` archive and binary;
  no core build or source mutation. Title and tab status use qualified OS hostname
  facts. Git projection uses authoritative roots/labels, opaque checkout identities
  and checked nullable counts. Ordinary linked checkouts keep herdr's independent
  rows; Git linkage does not establish Herdr-managed group membership.
- Native six-size M2 passes 154 checkpoints, MOK 33, repository 18 and explicit
  hostname status/title 33. Layout/reload passes 256 default, 280 configured and 45
  previous-only checkpoints. These include complete restored frames; all 13 layout
  runs prove two-client convergence. All 17 Settings notice checks remain
  pending-feature, never actual Settings overlay passes.
- Missing Git, nonexecutable Git and detached HEAD match at 120x40. Promisor true,
  false and empty each retain five raw count-cell differences, passed:false/exit1:
  core omits counts by policy while herdr computes 2/1. Full-frame known/unknown
  source controls explain the difference without masks; these are not passes.
- All 390 paired input cases and 14 positioned tails match. Four opt-in integration
  tests pass on `.8`. Native ownership verifies exact negotiated kind7 release
  bytes, child delivery without stealing phone 40x20, passive recovery, fresh
  observer output, phone-detach fallback and real allocation reclamation.
- Native findings fixed automatic visual grouping of ordinary linked checkouts
  (b5b5c94) and the Settings capture's obsolete comparison argument (abe69c6).
  Both have independent review, full gates, clean leak scans, normal pushes and
  green CI. Gates pass 607 default Rust tests, 3 decoder tests and 24 Python safety
  tests; five Rust tests remain opt-in/ignored in the default suite. Input fixtures
  and original configuration are unchanged; all owned native resources clean up.
  The disk guard kept measured free space above the 12 GiB floor.
- M2.2 stops for review. Managed Herdr worktree-group membership is not supplied
  by C3 and is not guessed; that presentation remains unproven. Appearance on
  first attach/reconnect/eviction and the acknowledgement consumption-fence race
  remain documented. M3b, Settings overlay and other M4 work are not started.

## M3b: Authoritative completion and attention (2026-11-06)

- C3 per-record completion markers now drive Done; initial idle/done is
  readiness. Client-local acknowledgement retains the focused fresh-paint
  gate and baselines origin/epoch/scope or incarnation changes. Turns,
  timestamps and observed transitions cannot manufacture completions.
- Priority recency preserves state/completion ordinal order within each
  activity domain. Incomparable domains merge local receipt order with
  deterministic ties; raw counters are never global across machines.
- `quiet_human_release: null` now uses DATA fallback. Managed worktree
  groups are explicitly `pending-feature` for M4 with shared membership.
- Native chronology matches all six sizes (114 checkpoints) plus a WezTerm
  profile (19), including another-tab attention, acknowledgement on visit,
  actual core epoch restart with retained markers and subsequent completion.
  M2/M2.2/M3a regressions, 390 paired input cases, 14 positioned tails,
  four `.8` integrations and full local gates pass. Promisor policy differences
  and Settings remain explicit exclusions; no expected bytes are changed.
- Independent reviews, leak scans and CI gate the changes. The stalled-output
  fence gap, first attach/reconnect/eviction appearance, native program OSC12,
  reporter labels and remaining M4 actions stay deferred. Pause for review;
  M4 is not started.

## Shared remote attach connection (fix branch `fix/shared-attach-conn`, unpushed)

- A remote endpoint's attaches now multiplex onto one framed `hc connect`:
  `AttachConn` is a lazy single-flight registry — the first open connects,
  concurrent opens share one transport and one `hello`, the first open
  after a death rebuilds once, and the last attach's end closes it. The
  shared reader routes through a channel table (held slots for pre-install
  frames, dead slots for late ones); the writer keeps control frames and
  owed credit first and serves stream input fairly across channels.
- Detach and drop close only their own channel — a synchronous CLOSE from
  `Drop` prevents a leaked viewer on the shared connection — and a
  cancelled open's late attach reply still gets its channel CLOSED.
  Per-channel violations (credit overflow, malformed stream data) end
  only that attach; a dead transport or channel-0 violation ends every
  attach through the existing error path.
- Local Unix sockets keep `Attach::connect`; watch and RPC streams keep
  their own connections; every connection to a target still shares the
  one ControlMaster login.
- Gates: formatting, all-target Clippy and the full test suite pass,
  including new fake-core shared-connection suites and a fake-ssh proof
  that two opens spawn one child and one `hello`. The real-core
  integration suite gained a two-attaches-one-connection case.

## M6: Release tooling and install path (2026-11-09)

- Release targets: `x86_64-unknown-linux-musl` and
  `aarch64-unknown-linux-musl` (static, Zig cross-builds libghostty-vt
  for the Cargo target) plus `aarch64-apple-darwin` and
  `x86_64-apple-darwin`. Portability fixes: `libc` names no
  TIOC*WINSZ/TIOCSCTTY constants on Apple targets — the fixed Darwin
  values are supplied where needed and the PTY smoke fixture uses
  `openpty`/`login_tty`; the deprecated `libc::time_t` alias is an
  explicit `i64` now.
- CI grows a macOS job (build all targets, `cargo test`,
  `scripts/test-install.sh` for the shasum fallback), and
  `.github/workflows/release.yml` builds all four targets on a `v*` tag,
  packages `hc-tui-<version>-<target>.tar.gz` (binary + LICENSE + NOTICE
  + short README), writes `sha256sums.txt`, and opens a **draft** GitHub
  release — publishing stays manual.
- `scripts/install.sh` (POSIX): platform detection with a clear
  unsupported message, latest release or `HC_TUI_VERSION`, sha256
  verification that installs nothing on mismatch, atomic install into
  `${HC_TUI_INSTALL_DIR:-$HOME/.local/bin}`, PATH and `hc` reminders.
  `scripts/test-install.sh` covers it offline — file:// fixtures and
  stubbed `uname`/`curl`; it also runs in CI on both Linux and macOS.
- Verified locally: the x86_64 musl build is `static-pie linked` (ldd
  agrees), `--version`/`--help` run, and a packaged archive installs
  through the real script path. The aarch64 musl build is checked on
  CI's `ubuntu-24.04-arm` runner — native, after `zig cc` as linker
  proved incompatible with rustc's `-Wl,--fix-cortex-a53-843419`
  erratum flag. Both Apple targets are exercised by CI only.
- Docs: README leads with Install; new `docs/install.md`,
  `docs/remote.md` and `docs/troubleshooting.md`. The first tag/release
  publication is pending — the user decides when it goes out.

## v0.1.1: one-command install and start (branch feat/one-command)

- `hc-tui` starts a local core itself when the resolved socket answers
  nothing (missing or refused), once per run, after terminal setup and
  without blocking the UI. It never starts one for `--remote` or a set
  `HC_SOCKET`; the watcher reconnect picks the new core up. Failures
  show a notice: `hc isn't installed` with the official install
  command, or `Core start failed` with the first error line.
- `scripts/install.sh` now runs the official `hc` installer when no
  `hc` is found on `PATH`, in the install dir or in `~/.local/bin`.
  `HC_TUI_SKIP_CORE=1` skips it; a failed core install warns and keeps
  hc-tui.
- `.github/workflows/pages.yml` publishes `scripts/install.sh` from a
  published release tag to Pages, with a minimal index; the short URL
  is not documented until a release ships.
- Gates: formatting, all-target Clippy, the full suite (8 `core_start`
  unit tests, 6 PTY tests driving a fake `hc`), 28 offline installer
  tests and parsed workflow YAML all pass. On an owned scratch session
  a core started by hc-tui (`hc 0.1.18-followcat.1`, `serve` pid
  1013562) still answered `hc server status` after detach and was
  stopped with `{"stopped":true}`.

## Hidden panes keep their size (branch fix/keep-hidden-size)

- A pane's silent size lease now lasts as long as its observer stays
  cached (user decision, 2026-11-09), so switching away and back no
  longer resizes the program. Peek panes stay strictly observe.
- Gates: formatting, all-target Clippy and the full suite (855 passed,
  7 ignored). On an owned scratch session, 30 switches between three
  stand-in agents painted no intermediate screen, and each agent was
  resized at most twice in the whole run.
- An inline stand-in with a footer shows the layout side of this. The
  build from before the lease fix gave two of the three an 80x24 size on
  about 17 of 30 switches, and one was left with blank rows under its
  footer. With this build they were only ever given the pane's size
  after the first attach, and no blank rows remained.

## macOS: detach in a PTY (known issue, 2026-11-09)

- On macOS the six `autostart` PTY tests time out: after `Ctrl+B q`
  hc-tui is still running. A stack sample on the CI runner shows the
  main loop and the input thread idle in `kevent` and `poll`, so it
  isn't stuck on the way out. The cause isn't known, and no real macOS
  terminal has been checked. The input code is the same in v0.1.0.
- Those tests are ignored on macOS for v0.1.1 (user decision). The
  issue is listed in `docs/troubleshooting.md` and the release notes.
- Commit `be10620` blames a full PTY buffer. That guess was wrong:
  draining the PTY didn't change the result. The draining stays, since
  a test shouldn't leave the child blocked on output.

## Programs can copy through hc-tui (branch fix/forward-osc52)

- A program's OSC 52 write used to stop at the pane's VT. Codex selects
  with the mouse and copies this way, so it reported a copy while the
  clipboard stayed as it was. hc-tui now takes the write from the VT,
  through the same library hook the reference uses, and re-emits it to
  the host.
- Each copy is written once: observers and popups count, the focused
  pane's input wire doesn't, since it carries the same bytes. Writes
  inside a paint are dropped.
- Gates: formatting, all-target Clippy, the full suite (858 passed, 7
  ignored) and 29 installer tests. On an owned scratch session, three
  copies from two stand-in agents, with switches between them, reached
  the outer terminal once each. The build before this change wrote none.

## Clipboard image paste on remote endpoints (branch feat/image-paste)

- `Ctrl+V` now reads the local clipboard image and uploads it to the
  program's machine with `terminal.put`, then pastes the returned path
  into the pane through the normal paste path. It applies on
  `--remote`, on hc-tui itself running over SSH, and on a focused
  terminal on another machine (`machine/tN`) — matching the reference's
  `endpoint_accepts_local_images`. On a plain local endpoint `Ctrl+V`
  still reaches the program unchanged, and `Ctrl+Shift+V` stays text
  paste everywhere.
- The other two reference triggers come along: an empty bracketed paste
  (a terminal's image-only `Ctrl+Shift+V`) and a paste holding one
  absolute image path (a file drop, including quoted and escaped
  forms). On Linux the read tries WSL PowerShell, `wl-paste` under
  Wayland and `xclip` under X11 in the reference's MIME order; each
  tool is killed after about two seconds and a missing tool counts as
  no image. macOS uses `osascript`. Payloads over the 8 MB
  `terminal.put` limit are consumed with a notice and never reach the
  wire. A key release for a consumed press is suppressed on hosts that
  report releases, and the suppress set only grows on those hosts.
- Gates: formatting, all-target Clippy, the full suite (890 passed, 8
  ignored) and 29 installer tests. The parity vectors replayed all 390
  input cases on reference and candidate with zero failures, and the
  80x24 run matched on every checkpoint except the documented navigator
  footer-identity cells. On an owned scratch session with a fake `ssh`
  running `hc connect` locally and a fake `wl-paste` serving a PNG, the
  ignored end-to-end test uploaded through the real `terminal.put` and
  the pasted path landed in the program's stdin.

## PgUp and PgDown page panes (branch fix/page-keys)

- Plain `PageUp` and `PageDown` reached the program as `ESC [ 5 ~` and
  `ESC [ 6 ~` and never moved the pane's own scrollback. The reference
  pages its scrollback by one pane height for those keys in a normal
  pane: main screen, no mouse reporting, and not a bare application
  cursor program without bracketed paste.
- hc-tui now applies the same rule per key event at the pane route.
  Presses and repeats page the displayed pane's scrollback by its own
  height, their releases stay swallowed, and every other case falls
  through to encode and forward. The scroll lands on whatever surface
  holds the screen, so a fresh-painted observer during reattach pages
  too. A key the program receives still jumps the view back to the
  bottom. Popup terminals are unchanged: their programs get every key.
- Gates: formatting, all-target Clippy, the full suite (867 passed, 7
  ignored), 29 installer tests, all 13 reference vectors including 14
  new page-key cases captured from the pinned binary, and the 80x24
  native parity run (only the documented navigator identity cells
  differ). On an owned scratch session both clients paged the same
  scrollback region while the fixture program received nothing.
