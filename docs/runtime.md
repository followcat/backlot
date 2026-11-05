# Read-only runtime

Start a local client with `hc-tui --session <name>`, or use
`hc-tui --remote <ssh-target> --session <name>`. The remote machine needs
`hc` on its SSH command path. SSH authentication and host verification use
normal SSH configuration; hc-tui does not store credentials. ControlMaster
can reduce the cost of the separate RPC, subscription and attach connections.

The M2 default prefix is Ctrl+B. `q` detaches, `w` opens workspace
navigation, `b` toggles the sidebar and `?` opens searchable keybind help.
Navigation uses the configured movement keys, Enter to select and Escape
to cancel. Herdr's configuration supplies the prefix, bindings, theme,
sidebar, tab bar and pane chrome. M3 implements layout and the remaining
mode bindings; M4 implements actions and custom command execution.

Only the focused terminal has an active attach. An observer never sends
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
