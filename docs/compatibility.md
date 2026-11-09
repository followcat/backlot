# Core compatibility

hc-tui runs on official `hc` (herdr-core) and on the followcat fork. Each
machine's capabilities are decided **per core** from that core's own `hello`,
so one sidebar can mix fork and official machines. A missing capability
renders blank or falls back — never an error, a retry loop or a stuck load.

| Feature | Fork `hc` | Official `hc` | On official `hc` |
|---|---|---|---|
| Protocol generation 1 (framed + plain) | yes | yes | Same |
| `repository_facts` — Git repo/branch in the sidebar | advertised | absent | Blank; the workspace label falls back to the cwd folder |
| `record_activity` — per-record completion ordinals | advertised | absent | Chronology falls back to `turn` + `herdr.seen` (below) |
| `os_hostname` — the machine's hostname fact | advertised | absent | The machine shows its `hello` name |
| `quiet_human_release` — attach flag | advertised | absent | Flag is not sent |
| Screen-derived agent status | present | absent | Agents show a status only when they report OSC 7501 |
| `automatic_input`, `resize_notifications` — attach results | may be absent | present | Tolerated; unknown result fields are ignored |
| Unknown fields and event kinds | — | possible | Ignored; optional fields stay `None`, unknown kinds skip |

## Done/unseen chronology without `record_activity`

- Fork: each record carries a completion ordinal; sidebar ordering and the
  done badge compare those ordinals.
- Official: the core keeps a per-terminal `turn` that bumps when the root
  status leaves `working`. `turn` is the only authoritative change marker,
  and it is never compared across terminals — each terminal gets its own
  singleton ordering domain.
- `herdr.seen` is a shared label: the highest turn any viewer acknowledged
  (herdr-app reads it the same way). `turn > herdr.seen` presents `Done`;
  `turn <= herdr.seen` presents `Idle`; lowering it (mark-unread) re-arms
  the badge.
- What differs on official: a terminal whose root reports `idle` without a
  prior `working` turn shows no completion (turn stays 0 — nothing
  finished). Fork-cores can record richer per-record history; the official
  fallback sees only the root's working→idle/done edges.

## Coverage

- `crates/hc-proto/tests/official_core.rs` — fake-core handshake and watch
  streams with fork features off: no-activity terminals, absent Git and
  hostname facts, official attach flags, unknown fields and event kinds,
  mixed machine sets.
- `crates/hc-tui/tests/real_core_smoke.rs` — the real `hc-tui` binary in a
  PTY against `HC_TUI_TEST_CORE`, asserting on the rendered screen. Opt-in;
  CI runs it on the pinned official build (see `docs/integration.md`).
