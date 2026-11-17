# Runtime

Start a local client with `hc-tui --session <name>`, or use
`hc-tui --remote <ssh-target> --session <name>`. The remote machine needs
`hc` on its SSH command path. SSH authentication and host verification use
normal SSH configuration; hc-tui does not store credentials. At steady state
`--remote` owns three `ssh` children per target: a dedicated plain-mode
`events.subscribe` stream (`Subscription`); one shared framed RPC `Client`
(`shared_rpc`) multiplexing the watcher's refreshes (`doc.get`,
`machine.list`, fallback `terminal.list`), layout and machine
`operations::Store` mutations and background `herdr.seen` label writes —
lazy, single-flight, rebuilt once when the wire dies; and one shared framed
`AttachConn` multiplexer carrying the active, observer and size-lease
terminal byte channels — opening with the first attach, shared by concurrent
opens, rebuilt once if it dies and closed with its last attach. Short-lived
`ssh` children overlap transiently while a reconnect or a graceful close
replaces a wire; before `shared_rpc` the same wiring ran up to five children
because refreshes, actions and seen writes each held their own connection.
All of these streams share one SSH login per target through hc-tui's own
control socket (`$XDG_RUNTIME_DIR/hc-tui-ssh`, typically
`/run/user/*/hc-tui-ssh`, created private): the OpenSSH control-master
daemon there persists for 10 minutes (`ControlPersist`) after the last
stream ends — it belongs to OpenSSH, is reused by hc-tui's children, and is
not itself an hc-tui child process.

The default prefix is Ctrl+B. `q` detaches, `w` opens workspace
navigation, `b` toggles the sidebar and `?` opens searchable keybind help.
Navigation uses the configured movement keys, Enter to select and Escape
to cancel. Herdr's configuration supplies the prefix, bindings, theme,
sidebar, tab bar and pane chrome. Ctrl+B then
Shift+R, or the mobile reload-config menu, rereads this client's herdr config.
Bindings, theme, chrome and allocated geometry update immediately. Invalid
sections keep their current values; valid siblings still reload. A malformed
or unreadable file keeps the current configuration and shows its diagnostic.
Manual sidebar width and agent ordering survive reload.

The focused terminal has the human-input connection. Persistent observers
retain the current tab and 32 most recent off-tab panes. An observer never sends
human input, automatic replies, size claims or host facts. A role change
opens a new attach. A recovered active attach uses `resumed` to avoid
claiming fresh activity merely because the transport recovered.

Active panes allocate a 256 KiB VT scrollback; observer panes cap it at
64 KiB. libghostty prunes scrollback at page granularity, so the effective
bound can exceed the configured cap. A zero cap cannot be used because
visible tabbed panes render and scroll from the observer VT when painted,
so setting observer scrollback to zero would drop visible pane history. Up
to 32 off-tab observers stay live in the LRU: tab switches back onto them
need no reattach and keep the one-shot cursor and palette state. An
observer evicted past the LRU drops its attach immediately — it receives
zero background feeds while evicted — and a revisit opens a fresh observe
attach that repaints from core history.

The allocated pane grid and rendered inner rectangle share one geometry.
When the authoritative core grid persistently differs, a source-style
notice offers a click to take control. Core does not identify the other
owner, so the message describes a possible competing viewer. An explicit
click sends a fresh resize request even when its dimensions did not change.

Broken connections retain the inventory and last pane paint, with a
visible notice. Recovery has bounded backoff and waits for the complete first-paint mode
marker before encoding queued semantic input. Attach reply incarnation
facts are checked before replay. An ambiguous disconnected batch is rejected
as a whole with a notice. Already encoded bytes are tied to their original
attach generation. Held-key releases remain bound to the original
wire across workspace switches; they never move to a replacement terminal.
Live viewer controls and human input retain FIFO order under backpressure.
A separate source-style ledger releases forwarded presses on graceful detach,
even when later UI bindings consumed the same key. Detach drains accepted
input and releases before the actual transport write
fence and CLOSE. Failures and rejected input use visible source-style notices.

The event loop waits for input, protocol events or a one-shot pending-work
deadline. Redraws are coalesced at 16.67 ms. A clean client has no periodic
redraw timer. Input framing, output queues, parked attaches and closing
routes have explicit bounds; shutdown owns cancellation and task reaping.
The terminal guard restores raw mode, keyboard/mouse reporting and cursor
policy on normal exit and error paths.

Host queries are accepted only in their corresponding reply windows.
Measured cell pitch is reported as FACTS; the reference's local 8x16
fallback is used for encoding when no pitch is measured, without claiming
that approximation as a measured core fact. See [parity](parity.md) for
initial-paint facts that the tested core revisions cannot supply.

All automated real-core validation creates random `hctui-test-*` sessions
under temporary HOME/XDG directories. See [integration](integration.md) and
[the parity harness](../scripts/parity/README.md). It never discovers or
connects to an existing session.

Visible panes keep silent size-holder connections, including the focused pane,
so a focus switch cannot leave a visible terminal without a size lease. These
connections send geometry and measured display facts, consume output credit
inside the attach actor — size-holder DATA never crosses the pane event
channel — and allocate no VT. They never send human input, focus or automatic replies.
A pane that leaves the visible layout loses its size holder. Zoom-hidden panes
retain pinned current-tab observers; off-tab observers enter the bounded LRU,
strictly observe-only. Resize requests follow layout
changes, without repeatedly fighting another viewer for ownership. A recovered
size holder uses passive `resumed`; recovery alone cannot take size from an
active phone viewer. When that viewer detaches, the remaining desktop viewers
restore their size automatically. Real tab entry, focus or allocation changes
can take sizing back. If allocation changes during attach, the requested size
is synchronized before focus even when another viewer already has that grid.

The default layout controls follow herdr: Ctrl+B then `c` creates a tab,
`v` splits right, `-` splits down, `h/j/k/l` focuses a neighbour, and
`H/J/K/L` swaps panes. `p/n` changes tabs, `1`–`9` selects a tab, `Tab`
cycles panes, `z` toggles zoom and `r` enters resize mode. In resize mode,
`h/j/k/l` or arrows adjust the split; Escape or Enter returns to normal.
`T` renames the tab, `P` renames the pane, `x` closes the pane and `X`
closes the tab. Closing the workspace's last tab asks for confirmation when
configured. Ctrl+G locks or unlocks keyboard passthrough. User bindings
replace defaults through the same herdr binding registry.

[Shared structure](layout.md) describes document ownership, CAS and adoption.
Live layout actions require an online inventory subscription. Missing core
methods or unknown owned document schemas produce a visible reason before
creation or close. Actions run on a bounded worker; only explicit CAS
conflicts retry. An ambiguous creation/write is never automatically replayed.

## Diagnostics snapshot

Set `HC_TUI_DIAG=<path>` when starting the client to make it explain itself.
Then `kill -USR1 <pid>` writes one JSON snapshot to `<path>`, and a run that
returns (detach, input closing or an error) writes a final snapshot before
exiting. Unset, the client installs no signal handler and touches no file.

```sh
HC_TUI_DIAG=/tmp/hc-tui-diag.json hc-tui --remote host-a
kill -USR1 "$(pgrep -f 'hc-tui --remote host-a')"
cat /tmp/hc-tui-diag.json
```

A snapshot is written to a temporary file beside `<path>` with mode 0600 and
renamed over it, so a reader never sees a partial file. A failed write is
dropped silently; diagnostics never change what the client shows. The writer
runs on its own thread, so a snapshot can still be taken while the loop is
busy or stuck. Pane counts are the loop's values at its last turn.

The snapshot has `schema` (currently 1), `uptime_ms`, and four sections:

- `loop.wakeups`: select-loop wakes by the arm that woke the loop, plus
  `total`. `none` counts channel-close wakes, which do no work.
- `loop.dirty_marks`: redraw requests raised while handling each wake kind.
- `loop.draws` and `loop.elided_draws`: frames written, and frames that
  matched the previous one and wrote nothing.
- `loop.projections` and `loop.cells_painted`: projection rebuilds and VT
  cells copied into pane surfaces.
- `loop.terminal_writes` and `loop.terminal_bytes`: write calls and bytes
  sent to the host terminal by frames and host-mode control sequences. The
  one-off startup and restore sequences are not counted.
- `loop.stdin_reads` and `loop.stdin_empty_reads`: reads on the input
  stream, and those that delivered no bytes (end of input or a spurious
  wake).
- `panes.by_role`: `active` (1 when the focused pane has a target),
  `observer` (persistent observers), `size_lease` (size holders) and
  `popup` (popups, peeks, command overlays and persistent floats).
- `panes.attach_calls_in_flight`: `terminal.attach` requests sent on the
  shared attach connection and not yet answered.
- `panes.channels`: attach channels that are `live` (installed), `held`
  (reply received, install pending) and `dead` (closed by this client,
  waiting for the core's close).
- `endpoints`: one entry, `index` 0, with `transport` `local` or `remote`.
- `connections`: one entry per connection kind, each with its `endpoint`
  index and `protocol` id (`events.subscribe` for `subscription`, `json-rpc`
  for `shared_rpc`, `terminal.attach` for `attach_mux`). They carry `reads`,
  `empty_reads` (reads that delivered no bytes, normally end of stream),
  `bytes_in`, `writes`, `bytes_out`, `connects` (established connections) and
  `reconnects` (establishments after the first).

The snapshot holds counters, gauges and protocol ids only. It has no terminal
text, titles, working directories, paths, host names, SSH targets, labels or
user names. Endpoints are keyed by index and transport kind, never by the
`--remote` target. A test runs a session whose fixtures contain those strings
and checks that none of them reach the file.

Not counted: dedicated attach connections opened outside the shared attach
connection, and connections made over injected test transports.
