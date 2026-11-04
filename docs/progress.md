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

## M1: Protocol client (in progress)

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
