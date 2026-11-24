# Core compatibility

hc-tui runs on official `hc` (herdr-core) and on the followcat fork. Capabilities are read from each core's own
`hello`, so one sidebar can mix fork and official machines. When a capability is missing, that part of the UI is left
blank or uses a fallback. It doesn't cause an error or a retry loop.

The fork's own contract with its clients — every fact below and how to detect it — is
[docs/fork-features.md](https://github.com/followcat/herdr-core/blob/v0.1.21-followcat.1/docs/fork-features.md).
A version suffix is never a capability check: `hello.result.features` and field presence decide.

## Versions

- **Built and tested against:** official `hc` 0.1.22 plus `hc ls --table` (`7cb4de47164f1d65db84dfab294166994f143a15`), the pin
  in [integration](integration.md), and the fork `0.1.21-followcat.1`.
- **Older official cores:** supported. Anything from 0.1.18 on works; a member or method a core does not know is
  simply absent and its feature stays off. Two examples: `events.subscribe`'s `docs` (0.1.21) is ignored by older
  cores, and `terminal.list`'s `machines` (always present from 0.1.21, omitted when empty before) reads the same
  either way.

| Feature | Fork `hc` | Official `hc` | On official `hc` |
|---|---|---|---|
| Protocol generation 1 (framed and plain) | yes | yes | Same |
| `repository_facts`: Git repo and branch in the sidebar | advertised | absent | Blank. The workspace label uses the cwd folder. |
| `record_activity`: per-record completion ordinals | advertised | absent | Chronology uses `turn` and `herdr.seen` (below). |
| `os_hostname`: the machine's hostname | advertised | absent | The machine shows its `hello` name. |
| `quiet_human_release`: attach flag | advertised | absent | The flag isn't sent. |
| Agent status read from the screen | present | absent | Agents show a status only if they report OSC 7501. |
| `automatic_input`, `resize_notifications`: attach results | may be absent | present | Accepted. Unknown result fields are ignored. |
| `put_chunks`: `terminal.put_begin`/`put_chunk`/`put_finish` | advertised | absent until 0.1.21 | The image paste sends one `terminal.put`. |
| `events.subscribe` `docs` (named documents) | n/a | 0.1.21 and later | Not named: hc-tui reads every terminal, and naming documents alone silences terminal events (and returns empty terminal snapshots from 0.1.22). |
| `terminal.list`'s `machines` | n/a | 0.1.21 always sends it | An empty list means no machines; older cores omit it, which reads the same. |
| `terminal.send` error kind `cancelled` | n/a | 0.1.21 and later | Older cores never send it; when it arrives the send is reported as refused, nothing was typed. |
| `restore_screen`, `restart_environment` | n/a | 0.1.21 (config, capability) | Not used: hc-tui keeps its own lost-terminal placeholder and offers no core restart. |
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
