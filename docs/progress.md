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
