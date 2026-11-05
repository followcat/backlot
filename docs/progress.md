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

## M2: Read-only terminal UI (joint review pending)

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
- M2 stops for the joint review. See [parity](parity.md) for exact tested-core
  limitations and the [runtime](runtime.md) and
  [harness](../scripts/parity/README.md) for usage.
  M3 layout/shared documents/full bindings and M4/M5 actions/copy features
  remain outside this implementation. No release is published.
