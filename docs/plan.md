# hc-tui plan

Each milestone ends with green CI and a short entry in `docs/progress.md` (done, measured, open).
Commits stay small and reviewable, with each change under about 400 lines when possible.

Herdr parity is a hard acceptance requirement. The reference is herdr
0.9.5-followcat at `d00953e0`, with the user's herdr config read by default.
Port its UI/config/input code directly with attribution; adapt core data rather
than inventing presentation. Record every data mismatch and exact replacement
in `docs/parity.md`. Same-size/config/fixture screens must match cells and
attributes, and the same key/mouse scripts must have the same effects.

| | Milestone | Acceptance |
|---|---|---|
| M0 | Bootstrap: Cargo workspace (`hc-tui` binary, `hc-proto` crate), CI, AGENTS.md, NOTICE | `cargo test` passes in CI; `hc-tui --version` |
| M1 | Protocol client: typed JSON-RPC over the Unix socket and over `ssh … hc connect`; `hello`/`rpc.discover` checks; `events.subscribe` with `missed` handling; framed-mode channel for attach; reconnect | Unit tests against a scripted fake core; integration test against a source-built `hc` in a scratch session (list, subscribe, run, read, kill) |
| M2 | Read-only UI with herdr's exact sidebar/pane presentation, switching keys and resize behavior; live counters/machines; one attached VT pane; detach/switch and observe; baseline herdr config; committed scratch parity harness | Over SSH: live terminal status, shell input and reconnect. Sidebar snapshots plus PTY parity at 80x24, 120x40 and 200x50, with key/mouse replay and cell/attribute diffs; M2 parity table in pause review |
| M2.1 | Joint-review remediation: 65/64/40-column native fixtures; strict readiness, full host mode/cursor restoration and positioned detach drains; Git/upstream/worktree/subdirectory fixtures | Available-fact screens match; missing Git facts remain `pending-core-fact`, never passes; independent read-only review before every commit, full gates, leak scan, normal push and green CI |
| M2.2 | Adopt the separate C3 core facts after their independent review and Claude approval | Remove the corresponding hostname, Git and quiet-release gaps using authoritative core facts; rerun fixtures before exact M2 acceptance |
| — | **Pause for review** (Claude + Codex + the user) | Review notes addressed |
| M3 | Structure: herdr-app reader with label/cwd fallback; shared `hc-tui` splits/focus/zoom/tabs; herdr's complete default/configured bindings, layout, tabs and lock | Same spaces/tabs as herdr-app; two clients share layout; pinned herdr behavior, default and user-configured bindings pass the parity harness |
| M3a | Approved independent slice: structure reader/shared layout, layout/tab bindings and operations (new tab, split, close, rename), Ctrl+G lock, current-tab persistent VTs with silent visible-pane size leases, plus 32 strictly observe-only off-tab LRU entries | Native parity at 80/120/200 and 64/40 columns, two-client CAS convergence, independent reviews, green CI and M3a review pause; no M4 work |
| M3a.1 | Accepted M3a follow-up: live config reload, Settings unavailable notice, passive size-holder recovery, explicit sizing/close/cursor-color docs | Reload at all six sizes, default/configured layout profiles, M2 regressions and owned phone/desktop recovery; independent reviews, green CI and M3a.1 pause; no M2.2/M4 |
| M3b | Completion chronology and dependent attention/recency after C3 approval | Authoritative core ordinals; local acknowledgement and deferred reporter labels remain separate |
| M4 | Actions: new terminal or agent with labels, split, close, rename, seen, keep, send, interrupt, worker peek, machines add/connect/rename/remove; notifications | Each action covered by a fake-core test; manual run on a real core |
| M5 | Copy mode and selection ported from herdr (OSC 52), popup selection, clipboard image paste through `terminal.put` | Copy/paste work locally and over SSH in kitty and one other terminal; copy-mode visuals/input pass parity harness |
| M6 | Release: musl builds, checksummed install script, docs (install, keys, remote, troubleshooting) | Tagged release with binaries; install script verifies the sha256 |

## Testing rules
- Integration tests use an `hc` built from the herdr-core source tree at a pinned commit:
  - with `HC_SESSION=hctui-test-<random>`;
  - with temporary `XDG_STATE_HOME`, `XDG_CONFIG_HOME` and `XDG_RUNTIME_DIR`;
  - stopped and cleaned up at the end.
- They never connect to a user's default or named sessions.
- Fixtures contain neutral data only: `host-a`, `alice`, `/home/user`, `acme-shop`.
- Parity runs also use an explicitly owned herdr scratch session, temporary HOME/XDG
  directories and a private copy of the user's config; never modify the original config
  or connect to existing herdr sessions. Commit only neutral harness/scripts/fixtures.
- Keep private captures outside the repository. Include M2 surface parity results,
  documented differences, measurements and remaining questions in the M2 pause review.
