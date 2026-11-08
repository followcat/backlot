# Runtime

Start a local client with `hc-tui --session <name>`, or use
`hc-tui --remote <ssh-target> --session <name>`. The remote machine needs
`hc` on its SSH command path. SSH authentication and host verification use
normal SSH configuration; hc-tui does not store credentials. On Unix the RPC,
subscription and attach connections share one SSH login per target through
hc-tui's own control socket (`$XDG_RUNTIME_DIR/hc-tui-ssh`, created private),
kept for 10 minutes after the last connection ends.

The default prefix is Ctrl+B. `q` detaches, `w` opens workspace
navigation, `b` toggles the sidebar and `?` opens searchable keybind help.
Navigation uses the configured movement keys, Enter to select and Escape
to cancel. Herdr's configuration supplies the prefix, bindings, theme,
sidebar, tab bar and pane chrome. M3a adds layout/tab navigation, new tabs,
splits, close, rename, resize mode and Ctrl+G lock. Custom commands,
notifications and the remaining mutation actions belong to M4. Ctrl+B then
Shift+R, or the mobile reload-config menu, rereads this client's herdr config.
Bindings, theme, chrome and allocated geometry update immediately. Invalid
sections keep their current values; valid siblings still reload. A malformed
or unreadable file keeps the current configuration and shows its diagnostic.
Manual sidebar width and agent ordering survive reload. Settings currently
shows a transient unavailable notice; its overlay belongs to M4.

The focused terminal has the human-input connection. Persistent observers
retain the current tab and 32 most recent off-tab panes. An observer never sends
human input, automatic replies, size claims or host facts. A role change
opens a new attach. A recovered active attach uses `resumed` to avoid
claiming fresh activity merely because the transport recovered.

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
connections send geometry and measured display facts, consume output credit,
and allocate no VT. They never send human input, focus or automatic replies.
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
