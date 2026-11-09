# Core compatibility

hc-tui runs on official `hc` (herdr-core) and on the followcat fork. Capabilities are read from each core's own
`hello`, so one sidebar can mix fork and official machines. When a capability is missing, that part of the UI is left
blank or uses a fallback. It doesn't cause an error or a retry loop.

| Feature | Fork `hc` | Official `hc` | On official `hc` |
|---|---|---|---|
| Protocol generation 1 (framed and plain) | yes | yes | Same |
| `repository_facts`: Git repo and branch in the sidebar | advertised | absent | Blank. The workspace label uses the cwd folder. |
| `record_activity`: per-record completion ordinals | advertised | absent | Chronology uses `turn` and `herdr.seen` (below). |
| `os_hostname`: the machine's hostname | advertised | absent | The machine shows its `hello` name. |
| `quiet_human_release`: attach flag | advertised | absent | The flag isn't sent. |
| Agent status read from the screen | present | absent | Agents show a status only if they report OSC 7501. |
| `automatic_input`, `resize_notifications`: attach results | may be absent | present | Accepted. Unknown result fields are ignored. |
| Unknown fields and event kinds | n/a | possible | Ignored. Optional fields stay `None`, and unknown kinds are skipped. |

## Done and unseen without `record_activity`

- On the fork, each record has a completion ordinal. Sidebar ordering and the done badge compare those ordinals.
- On official `hc`, the core keeps a per-terminal `turn` that goes up when the root status leaves `working`. Turns are
  never compared across terminals.
- `herdr.seen` is a shared label holding the highest turn any viewer has acknowledged. herdr-app reads it the same way.
  If `turn > herdr.seen` the terminal shows `Done`, otherwise `Idle`. Lowering the label marks it unread again.
- A terminal that reports `idle` without an earlier `working` turn shows no completion on official `hc`, because its
  turn is still 0. The official fallback only sees the root status going from working to idle or done. The fork can
  also record history per record.

## Test coverage

- `crates/hc-proto/tests/official_core.rs` uses a fake core with the fork features off. It covers terminals with no
  activity, missing Git and hostname facts, official attach flags, unknown fields and event kinds, and mixed machines.
- `crates/hc-tui/tests/real_core_smoke.rs` runs the real `hc-tui` binary in a PTY against `HC_TUI_TEST_CORE` and checks
  the rendered screen. It's opt-in, and CI runs it on the pinned official build. See [integration](integration.md).
