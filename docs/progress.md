# Milestone progress

## M0: Bootstrap (2026-11-04)

- Implemented in `7bf3691`; supervisor review and the private-data scan passed.
- Added a Rust 2024 workspace with the `hc-tui` binary and `hc-proto` library.
- The CLI provides help, version, and an honest bootstrap message. It opens no
  core connection; the interactive UI is not implemented yet.
- Added CI for formatting, all-target Clippy with denied warnings, and tests.
- Validation: the three checks pass locally; six tests cover the supported
  protocol generation and CLI behavior using temporary XDG directories.
- GitHub CI passed for the bootstrap commit.
- Open: protocol transport and scratch-core integration arrive in M1. Startup
  performance and live UI behavior have not been measured.

## M1: Protocol client (in progress)

- Task 1 (types and wire codecs): hc-proto now carries the typed params and
  results for the methods hc-tui uses, strict JSON-RPC 2.0 envelopes with
  structured `data.kind`/`hint` errors, a hello capability helper, and the
  bounded line and framed codecs over a shared read-ahead buffer. Conformance
  is tested against a pinned, sanitized copy of the OpenRPC schema
  (herdr-core `ec8adcdf`). The wire/envelope layer landed in `5823230`; the
  typed layer follows as a second commit. Validation: the three checks pass
  locally with 59 tests, and supervisor review passed on the combined work.
  Transport, endpoint discovery, SSH and a real scratch core are still open
  tasks.
