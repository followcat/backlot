# hc-tui plan

Each milestone ends with green CI and a short entry in `docs/progress.md` (done, measured, open).
Commits stay small and reviewable, with each change under about 400 lines when possible.

| | Milestone | Acceptance |
|---|---|---|
| M0 | Bootstrap: Cargo workspace (`hc-tui` binary, `hc-proto` crate), CI, AGENTS.md, NOTICE | `cargo test` passes in CI; `hc-tui --version` |
| M1 | Protocol client: typed JSON-RPC over the Unix socket and over `ssh … hc connect`; `hello`/`rpc.discover` checks; `events.subscribe` with `missed` handling; framed-mode channel for attach; reconnect | Unit tests against a scripted fake core; integration test against a source-built `hc` in a scratch session (list, subscribe, run, read, kill) |
| M2 | Read-only UI: sidebar (spaces and agents views, counters, machines) from live events; one focused pane attached and rendered (VT emulation); detach and switch; resize; observe for unfocused panes | Over SSH to a box running hc: see every terminal with live status, attach, type in a shell, survive a reconnect. Snapshot tests of the sidebar |
| — | **Pause for review** (Claude + Codex + the user) | Review notes addressed |
| M3 | Structure: herdr-app document reader with label and cwd fallback; the `hc-tui` layout document (splits, focus, zoom); tabs bar; herdr-like default keys; `Ctrl+G` lock; key config file and herdr config import | Same spaces and tabs as herdr-app on the same core; two hc-tui clients share a layout; keys match herdr 0.9 defaults |
| M4 | Actions: new terminal or agent with labels, split, close, rename, seen, keep, send, interrupt, worker peek, machines add/connect/rename/remove; notifications | Each action covered by a fake-core test; manual run on a real core |
| M5 | Copy mode and selection (OSC 52), popup terminal with selection, clipboard image paste through `terminal.put` | Copy and paste work locally and over SSH in kitty and one other terminal |
| M6 | Release: musl builds, checksummed install script, docs (install, keys, remote, troubleshooting) | Tagged release with binaries; install script verifies the sha256 |

## Testing rules
- Integration tests use an `hc` built from the herdr-core source tree at a pinned commit:
  - with `HC_SESSION=hctui-test-<random>`;
  - with temporary `XDG_STATE_HOME`, `XDG_CONFIG_HOME` and `XDG_RUNTIME_DIR`;
  - stopped and cleaned up at the end.
- They never connect to a user's default or named sessions.
- Fixtures contain neutral data only: `host-a`, `alice`, `/home/user`, `acme-shop`.
