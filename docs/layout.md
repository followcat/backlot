# Shared structure and layout

Structure is resolved per machine from the read-only `herdr-app` document,
then `herdr.space`/`herdr.tab` terminal labels, then cwd. Foreign versions and
unknown fields are tolerated; unsupported mod content is ignored individually.
Floating terminals retain their space/tab membership and are tiled in the TUI.
Raw length-prefixed identities keep display truncation and punctuation out of
identity. Remote addresses and document keys retain their machine qualification.

Only the connected core's `hc-tui` document is written. Schema 1 stores terminal
address trees, focus/previous focus, zoom, tab names/order, active tabs, closed
incarnations and adoption agreements. Unknown document/tab fields survive
writes; unsupported schemas and unknown tree fields block mutations. Inventory
and foreign structure remain authoritative for membership. Missing members join
saved trees, stale/cross-tab members are pruned for display, and closed records
suppress the matching inventory incarnation.

Each edit reads the latest document and uses versioned `doc.set`. Explicit CAS
conflicts reread and reapply the intention, up to eight attempts. Concurrent
splits and unrelated opaque fields survive. Split drags carry the original
topology so a collapsed tree cannot redirect a positional path. A confirmed
write is retained over older watcher publications and fenced to the core ID.
Shared focus follows subsequent document publications without echo writes.

Creating a tab or split uses `terminal.run`, with `herdr.space`, `herdr.tab`
and `herdr.seen=0` labels. Opaque label identities for cwd-only groups are
agreed by CAS before promotion. Concurrent clients reuse that agreement;
old layout keys redirect to the adopted identities. Names and existing label
fields are preserved. The foreign `herdr-app` document is never rewritten.
Placement and method/schema compatibility are checked before spawning.

Closing records a tombstone and prunes the tree in the same CAS write. Focus
stays in a surviving pane, then moves to the adjacent tab. Close confirmation
captures membership and core identity; submission checks the current view,
and the worker reads the terminal and foreign membership again before kill.
These are separate protocol calls, not an atomic core transaction. Partial
close or an ambiguous transport failure is reported; mutation requests are
never automatically replayed on a replacement connection.

Current-tab VTs stay attached; 32 off-tab VTs form an LRU. Every geometrically
visible pane also holds a silent size lease. Hidden zoom panes and off-tab LRU
panes retain only their observe connection. Observers and silent size holders
never send human input or focus. Visible size holders reacquire their permitted
lease after a transport recovery; recovered active input connections retain
the protocol's passive `resumed` behavior. Already forwarded held-key releases
can still drain on their original parked input wire. See [parity](parity.md) for appearance limitations and
the client-local acknowledgement/older-stream stall race.
