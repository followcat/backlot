# Herdr parity

The acceptance target is **herdr 0.9.5-followcat**, source revision
`d00953e081389eb5f8ea006896fbf4a17b2f9dfc`. With the same configuration,
terminal size, programs and states, matching surfaces must produce the same
cells, glyphs, colors and attributes, and react the same way to keys and
mouse input.

**Validation status:** the joint M2 review did **not** accept the shell as
indistinguishable. M2.1 tightens the harness and covers the default 64-column
mobile boundary. M2.2 adopts the reviewed C3 facts and validates the explicit
fixtures against pinned `hc 0.1.17-followcat.8`. Baseline, Git, layout, reload,
input and quiet-release results are recorded below; promisor policy differences
are retained, never passes. This is not a claim about every source feature. C3 will not supply
first-paint program appearance: M3a retains VTs
for cached switching and documents first attach, reconnect and eviction as
known limitations. A fixture with missing core facts is `pending-core-fact`,
never a pass. Earlier passes establish only their explicit captured fixtures.

## Source and configuration

Port the reference UI, config and input code directly, keeping source-file
attribution and Apache-2.0 notices. Relevant paths include `src/client/shell.rs`,
`src/client/shell/`, `src/ui/`, `src/config/`, `src/input/`, `src/raw_input.rs`
and `src/pane/input.rs`. Use the thin client's actual behavior at the pinned
revision when similarly named older server-side UI code differs.

The baseline configuration is herdr's `config.toml`, normally
`~/.config/herdr/config.toml`, with herdr's parsing and defaults. Reading it
does not migrate or rewrite it. Any hc-tui-specific override must overlay
that baseline. Parsing a binding does not make an action outside the current
milestone available.

## Milestone coverage

| Milestone | Required parity |
|---|---|
| M2 | Sidebar and endpoint rows, pane cells/attributes/cursor, titles, switching/navigation, resize, applicable mouse behavior, baseline configuration, scratch PTY harness |
| M3 | Herdr-app structure reader, shared splits/focus/zoom/tabs, complete default/configured bindings and lock |
| M5 | Copy mode, selection and clipboard behavior/visuals |

The M2 joint review pause stays in place. Later milestones do not excuse
incorrect behavior on an M2 surface. Popups, confirmations and other actions
must be checked when their implementation milestone adds them.

## Status vocabulary

Core record authority and herdr's display priority are separate decisions;
see [sidebar semantics](sidebar.md). After choosing the authoritative record,
adapt its word to the reference presentation:

| Core record word | Reference presentation |
|---|---|
| `working` | Working |
| `blocked` | Blocked |
| `idle` | Idle when acknowledged; Done after an unacknowledged completion |
| `done` | Idle without an unseen authoritative completion marker; Done with one |
| `ready`, `finished` | Legacy display aliases do not establish completion without a core marker |
| `error` or an unfamiliar word | Unknown; the reference has no separate error/unfamiliar-word status |
| No report | No agent status inferred from terminal contents |

The record's raw word and extensions stay in the inventory. The reference
renderer supplies status glyphs, colors, text and aggregate priority. A
record name, `source`, progress or blocked kind does not introduce a new
badge format. Unknown words must not be guessed to mean working or blocked.

## Core facts and replacements

These are specific data adaptations. They do not allow arbitrary screen
differences, broad regions to be ignored, or available data to be omitted.
The final harness must identify the exact fact responsible for an allowed
difference.

| Reference fact | Core facts and required replacement |
|---|---|
| Workspace/tab/layout identities | M2 uses raw `herdr.space`/`herdr.tab` labels and endpoint/cwd fallback, with distinct unlabelled tabs. M3 reads the herdr-app document and shares hc-tui layout. Display clipping never changes identity. |
| Agent runtime state | Use the authoritative root/named status record and the vocabulary above. Core supplies status; hc-tui does not add screen detection. |
| Manual, reported and OSC titles | All are available: `herdr.title`, status `title`, terminal `title`. Keep separate source fields. The optional core terminal `name` supplies the source name field. Title loss or swapping is a bug. |
| Shared completion acknowledgement | C3 still supplies no shared acknowledgement or output-consumption fence. Local acknowledgement resets/baselines on activity `(core_id, epoch, scope)` changes; `revision` alone does not reset it or advance recency. It is not synchronized or persisted across clients. |
| Global state-change ordinal | Optional C3 `activity.records[""].state_ordinal` and `completion_ordinal` are retained with `core_id`, `epoch`, `scope`, `revision` and named records. The maximum present clock orders comparable members of each incoming batch only; display receipts are allocated before watcher coalescing. Changed/new agents allocate from max current cache, unchanged retain, absent prune; foreign counters are never compared. Delayed A10 after A20/B displays A10 > B > A20. No timestamp/turn inference or permanent domain queues. See [chronology](#completion-identity-and-chronology). |
| Workspace branch | C3 `repository.branch`, displayed only for `state == "ready"`. Pending/stale/unavailable and future states retain their full facts but hide Git presentation; absence means unsupported. No viewer Git probe. Validated against the pinned `.8` fixtures. |
| Git ahead/behind counts | Nullable C3 `repository.ahead_behind`, with checked u64-to-usize conversion. Unknown stays absent, distinct from observed `(0,0)`. Promisor configuration, including false/empty values, deliberately suppresses counts. Source policy differences are recorded separately, never passes. Validated against the pinned `.8` fixtures. |
| Worktree identity and label | Logical C3 identity groups by `(repository.core_id, checkout.repository_key)` and distinguishes `checkout_key`; all keys/paths stay opaque. Ready facts retain names, root, linked and bare flags in neutral checkout metadata. Ordinary linked Git checkouts remain independent workspace/branch rows, matching herdr. Herdr visual groups require explicit `worktree_space` membership, which C3 does not supply; `pending-feature` (M4): managed groups require explicit shared `hc-tui` document membership tied to origin, repository, checkout and parent/child role; tree glyphs, collapse, aggregated attention and branch labels follow that membership. Git linkage alone never creates it. |
| Repository-root fallback label | C3 `checkout.repo_root` and `repository.auto_label` replace launch-cwd inference; explicit herdr-app/space/tab membership retains precedence. A plain-directory observation is distinct from an unsupported absent repository object. Validated against the pinned `.8` fixtures. |
| Workspace/agent tokens | No documented mapping for the source configurable token/context-usage values. Leave token collections empty. Unknown protocol extensions are retained without guessing their meaning. |
| Agent state labels | No documented equivalent for the source server's state-label collection. Leave it empty; core parent addresses or record sources are not state labels. |
| Worker relationship | Core `parent` is available and remains verbatim. The reference sidebar has no parent-note field; a worker uses an ordinary agent row with its own identity. Unknown parents do not hide it. |
| Endpoint state | Local connection/subscription state and remote `machine.list` states drive the reference endpoint enum. Offline snapshots remain listed. RPC-only loss retains streaming terminal reports and marks stale machine information; subscription loss makes the retained inventory stale. Use the source stale/dimming presentation. |
| Size-owner identity | Active/observe role, size and focus facts are available, but the tested protocol does not name another size owner. Preserve role and authoritative sizes, and omit an invented owner name. Visible ownership notices and explicit take-control behavior still require validation. |
| Hostname | Use optional `hello.core.hostname` only with `os_hostname`, and connected/fresh `MachineView.hostname` for remote endpoints. Both title and tab status use the fact; missing/unconfirmed/offline facts produce no hostname text. Never substitute configured `core.name` or a relay alias, or run discovery commands. Validated against the pinned `.8` fixtures. |
| Server clock and timezone | Core exposes no remote clock/timezone fact. Datetime tab status uses the client local clock with the source formatter and sanitization. Remote clock/timezone parity requires a protocol fact. |
| Earlier cursor-style request | The tested cores omit earlier DECSCUSR visual shape/explicitness from initial attach paint. Follow the precise limitation below. |
| Earlier program default-color/palette overrides | The same cores omit prior OSC 10/11/4 state from first paint. Use the measured host defaults/palette until live program updates arrive, as described below. |

The optional source fields above remain absent in the renderer's normal
absence paths. Core-specific connection and ownership notices must use the
source notice/error presentation; their final text and interaction remain
part of the pending M2 review.

## Endpoint-state mapping

The current data adapter uses the reference endpoint states below. Cached
terminal reports keep their own status while endpoint freshness changes.

| Core/watch facts | Reference endpoint state |
|---|---|
| Local subscription online or static, inventory fresh | Online |
| Local first connection attempt | Connecting |
| Local reconnect/offline or stale inventory | Reconnecting |
| Local watcher stopped | Disabled |
| Remote machine `connected`, transport metadata fresh | Online |
| Remote machine `connecting`, transport metadata fresh | Connecting |
| Other remote machine state, or an unlisted remote prefix | Attention |
| Subscription lost | Reconnecting for retained remote snapshots |
| Only RPC lost | Attention for retained remote snapshots; terminal reports continue |

Literal text, hostname and datetime tab-bar entries are projected using the
source spacing and sanitization. Command entries are parsed but their shell
execution belongs to M4; saved agent-view labels require the M3 structure
reader. These are scope boundaries, not unavailable-core exceptions or
permission to mask configured M2 output.

## Initial cursor-style limitation

This was measured against core revisions
`ec8adcdf0da38e539e8702f7c37801647da035f3` and
`3ed2e9170f7112c444661530044a98d7cd719ebf`. It is a statement about those
tested revisions, not every generation-1 implementation.

A scratch program requested each of: no cursor style, `CSI 2 SP q`,
`CSI 5 SP q`, and `CSI 5 SP q` followed by `CSI 0 SP q`. The first attach
paint contained no DECSCUSR sequence in any case. The blinking-bar request
only added `CSI ? 12 h`, which preserves blinking but cannot identify the
visual shape or distinguish an explicit request from the user default.

M3a retains observe-only VTs for every pane in the current tab, plus an LRU
of 32 off-tab panes. The focused terminal has a separate active input wire.
Rendering prefers the persistent observer's completed paint, so routine cached
switching preserves live cursor style, OSC 12 and OSC 4 state. The observer
connections send no input, resize, focus or display-fact claims. Each VT keeps bounded
256 KiB scrollback; the off-tab cache has a fixed entry budget. Current-tab
panes are pinned, so memory also scales with the explicitly displayed layout.

Visible current-tab panes also hold silent size leases, including the focused
pane, so focus switches have no gap without a visible size owner. These
controllers send size and measured display facts, consume credit, allocate no
VT and send neither human input, focus nor automatic replies. Leaving the tab
closes the controller; off-tab LRU connections remain strictly observe-only.
A visible controller recovers with passive `resumed`; it never claims focus
or sends input. Transport recovery alone does not reclaim another viewer's size.
This user-approved rule supersedes the earlier blanket observe-only rule for
visible split panes. One displayed geometry supplies rendering and both size
requests, including observer-loading and alternate-screen transitions.

First attach, reconnect and LRU eviction remain known limitations: the earlier
appearance is absent from the tested core paint. C3 will not add this fact.
These cases cannot be counted as exact parity before a fresh live program
request supplies the state. Native cached-switch parity is a separate M3a gate.

A client attaching or reconnecting after such a request cannot recover
that earlier visual shape from this paint. Until a live request arrives,
use the reference user-default policy and retain the available cursor
visibility, position and blink facts. Live DECSCUSR, reset-to-default and
unconfigured-program behavior must still match the reference. Initial
default style must not be replaced by a guessed block or bar.

Underline kinds, wide and combining cells survive core paint. Explicit
truecolor styling and ModifyOtherKeys2 also survive. Loss of those
attributes or keyboard modes is a client bug, not this limitation.

## Initial program-color limitation

The same two core revisions above were tested with program requests made
before attach: OSC 10 foreground `#abcdef`, OSC 11 background `#102030`,
and OSC 4 palette entry 1 `#405060`. Readiness was confirmed through a
program-set OSC title before attaching, so these requests had already
reached core.

Neither first paint contained the prior OSC 10/11/4 state. In these pins,
`crates/ghostty-vt/src/lib.rs` configures the first-paint formatter with
palette serialization disabled. Decoding the complete paint with the same
host seed used for UI comparison showed the replacement precisely:

- A character using palette entry 1 used the decoder's host/baseline entry,
  rather than the program's `#405060` override.
- Default foreground/background used the host/baseline values, rather than
  the program's `#abcdef`/`#102030` requests.
- An explicitly styled truecolor character retained its exact `#010203`
  foreground.

Live OSC 10/11/4 updates were preserved byte for byte, and the native decoder
then applied their actual values. This gap is limited to program color
state established before initial/reconnect paint; terminal inventory also
provides no corresponding color fields. A fresh VT uses the measured host
defaults/palette, or the reference absence behavior for unreported host
facts, until a live program update supplies the missing state.

Live overrides and resets must remain correct: a late host reply must not
erase a program override or promote it into a host default. Resetting the
program's override must reveal the latest host value. A parity case for
this initial-state gap must still compare text, geometry, attributes and
unaffected colors; it must not mask a whole pane.

## Required harness and review evidence

The committed harness must run both programs through real PTYs at 80x24,
120x40 and 200x50. It must:

- Create unique owned herdr and hc scratch sessions with temporary HOME/XDG
  directories, neutral programs and explicit endpoints; never attach to an
  existing/default session.
- Use a private copy of the reference configuration and leave the original
  unchanged. Do not invoke unrelated custom commands during fixtures.
- Replay the same key and mouse scripts, including switching, navigation,
  divider dragging, applicable scrolling, focus and resize.
- Compare text, wide/spacer cells, colors, attributes and cursor state with
  the same native VT decoder and host foreground/background/palette seed.
- Replay resize events in order. Capture checkpoints before detach and keep
  teardown output separate; core paint can itself begin with alternate-screen
  reset sequences, so stripping bytes at those sequences is invalid.
- Verify actual program input, including the 25 legacy and 25 inner-Kitty cases
  for each of the zero-capability and confirmed-Kitty host profiles
  (100 captured cases), 23 held/layout/live-mode/orphan cases, seven focus
  cases, 35 functional-key cases and 225 mouse cases: 390 total, all from
  the pinned reference in `crates/hc-tui/tests/fixtures/`. With the reference
  manifest, both clients replay every fixture and their post-case/detach
  input is checked too. Only synthesized CSI-u release ordering in the
  detach tail is compared as a multiset: the reference drains a HashMap.
  Release contents/counts and all other bytes remain significant.
- Restore terminal modes, stop and reap owned processes, and retain recovery
  evidence if verified cleanup fails. Keep private captures/logs/keys outside
  the public repository.

Reference-derived expected bytes or cells must not be regenerated from the
candidate to make failures pass. Record each mismatch and its exact cause;
fix every available-fact mismatch. The M2 pause review must contain the
surface parity table, actual measurements, final check/CI results, precise
limitations and remaining joint-review questions.

## Completion identity and chronology

M3b consumes the C3 `.8` per-record activity contract. Completion is a
non-null `completion_ordinal` on a present root/named idle/done record, not
an observed Working transition or a turn change. Initial idle/done is
readiness. Baseline existing markers on first epoch observation and lifetime
changes; ordinary reconnect retains client-local acknowledgement. Only the
existing focused, composed, fresh visible-pane render gate acknowledges.

Within `(core_id, epoch, scope)`, state/completion ordinals order comparable
members of a new ingestion batch. They do not constrain historical display
rank: late A10 after A20/B receives the newest receipt. Bounded watcher metadata
preserves receipts before a slow reader coalesces views. Reconnect, metadata,
revision, turns, timestamps and acknowledgement do not advance unchanged facts;
absent agents are pruned and allocation uses max current cache. Saturated ranks
tie stably. No foreign counters or permanent domain queues order agents.
Existing activity-domain baselines retain their receipt; reference-compatible
epoch display rebuilding remains pending as a separate patch, including new
post-epoch work arriving before a draw. See [the precise ordering rule](sidebar.md#attention-seen-state-and-order).

There is no shared acknowledgement or ordered output-consumption fence.
Inventory can precede stalled stream output, so that race remains an explicit
limitation. Native chronology captures require fresh fixture output and stable
post-reconnect frames; they do not claim stall-race, initial/reconnect appearance
or cross-client Done parity.

## Release activity and size ownership

Pre-C3 cores treat human release DATA as activity, which can reclaim size.
M2.2 opts in to `quiet_human_release` and sends frame kind 7 only when the
hello feature, attach request and affirmative attach reply all confirm it.
Keys and mouse releases are tagged before encoding; bytes are never inspected
to classify a release. The human input gate, FIFO, chunking and drain fence
remain shared with ordinary input. Observe and silent size-holder paths
cannot send human releases. Missing/false confirmation uses DATA, never
AUTOMATIC_INPUT, retaining the older core's activity policy.

Parked releases stay on their original negotiated wire; graceful synthetic
releases drain before CLOSE. Dead-wire releases are not synthesized/replayed.
The owned phone/desktop harness now checks exact kind 7 payloads during the
release interval, delivery to the child and retention of the phone's size.
The `.8` native ownership run confirms all three negotiation signals, the
exact kind 7 release payload during its own interval, child delivery and
retention of the phone's 40x20 allocation. It also proves passive recovery
of five cut desktop connections, fresh observer output, automatic fallback
after phone detach and reclamation on real allocation change. Fake-core
request/hello/reply combinations retain the unconfirmed DATA fallback.


## First paint, input ordering and recovery

The two pinned cores terminate initial attach paint with `CSI ?9001h/l`,
after the restored keyboard and paste modes. Human input waits for that
marker, including when paint and marker span DATA chunks. Viewer state is
synchronized before initial input; live focus/resize/facts and human input
then share a FIFO. The writer crosses a DATA fence before priority controls
and CLOSE. An actual resize during fragmented paint is applied at the gate.

Input never crosses terminal/incarnation identity. A new attach reply's
`created_ms` and pid are checked even if inventory is late. An ambiguous
disconnected batch is rejected as a whole with a notice; a dropped prefix
must not leave a later Enter eligible for replay. An undispatched key tap
is cancelled with its queued repeats on key-up or focus loss. New semantic
input accepted after loss waits for the recovered paint.

A local resize known to be unsent is retained across recovery. Core has no
acknowledgement for a resize accepted before a transport died; a resumed
attach cannot distinguish that from another viewer's later claim. It does
not repeatedly resize merely because it reconnected. The ownership notice
and explicit take-control action remain available.

## Graceful detach and dead-wire releases

Like the reference server, hc-tui retains forwarded key presses separately
from client shortcuts, then releases the remaining presses on graceful
detach. A later UI-consumed press does not erase an earlier delivered press.
Unencodable Hyper/Meta chords still update the source-style held-key ledger.
Healthy parked routes retain their original generation and current modes.
A forwarded press replaces the same code across all routes, as in the
source server; a UI-consumed key does not. A switch retains outstanding
server-held presses until real release, replacement or actual detach.

There is one narrower transport limitation: the reference server owns native
key leases and can synthesize releases when a client disappears. Core's DATA
stream carries bytes and does not maintain those native leases. If a wire
dies after a press reached the program, hc-tui cannot deliver the later
release on that dead wire. It never sends it to a replacement incarnation
or another viewer to conceal the loss. Undispatched taps, ordinary key-up,
healthy switched routes and graceful detach are not covered by this gap.

## Title and teardown comparisons

The native decoder includes outer title and host modes as well as every
cell's resolved underline color. All title templates compare exactly,
including the default `{hostname}: {workspace}`. M2.2 removes the old
core-name/OS-hostname exemption. No cells are masked. Complete restored
frames, including cells, cursor, modes and retained title, are also compared
before a run can report success.

Detach must restore raw mode, cursor visibility, line wrap, keyboard flags,
alternate screen, paste, focus, mouse and scheme reporting. The tested
reference retains its last window title on exit, so hc-tui follows that
measured behavior rather than inventing a title reset.


## M2.1 harness and action coverage

The default plain-directory run includes 80x24, 120x40, 200x50, 65x24,
64x24 and 40x24. Source switches to mobile layout at **64** columns. Both
mobile widths exercise keyboard and mouse switching, every fixture status,
the full switcher menu and its Help action. Requested status glyph/color,
selection highlight and return to terminal mode are explicit readiness
conditions. Menu Help preserves Navigate/report-all while the switcher is
suspended, matching source, then resumes the appropriate mode on close.

Resize requires a unique child-generated marker, emitted only after its
actual PTY grid matches the expected allocation. Decoder dimensions alone
cannot satisfy it. Host checks include legacy mouse modes9/1000/1002/1005/
1015 and ModifyOtherKeys0/1/2. Teardown restores the simulated host's default
Block/steady cursor, not just visibility. Only fixture-listed synthesized
release segments permit unordered drain; their byte positions relative to
focus and every other event remain fixed. Late ordinary input fails before
detach; bounded quiescence is not proof about arbitrarily delayed traffic.

`--scenario repository` creates an owned local Git repository, a locally
tracked upstream branch with two commits ahead and one behind, a linked
worktree and an unlabelled terminal in `src`. No network fetch occurs.
The default desktop checkpoint requires both clients to show `fixture-main`,
`↑2 ↓1`, `fixture-linked` and `acme-shop`. Core repository/activity observations
must settle and pass independent field assertions before starting its UI.
Additional cases remove Git from the owned PATH, supply a nonexecutable Git,
detach HEAD, and set promisor configuration to true, false or empty. There
is no network fetch or viewer repository probing. At65/64/40 columns the
fixture compares the workspace picker and return with the same keys.

All raw differences fail, even with a core-policy annotation; policy annotations
never waive unrelated rendering, cursor, mode or title differences. An exact
surface match with an explicit semantic policy difference remains
`core-policy-difference` (`passed:false`, exit3), never a parity pass.
The pre-C3 66-difference/exit2 evidence is historical. The `.8` standard
repository fixture now matches exactly at all six sizes, including complete
post-detach frames. Missing Git, nonexecutable Git and detached HEAD also
match exactly at 120x40. All three promisor variants retain five raw cell
differences on the count segment, `passed:false`, exit1. Their complete source
frames equal the standard known-count source control; their complete candidate
frames equal herdr's missing-Git unknown-count control. No cells, cursor, modes
or title are masked. These are documented core-policy differences, not passes.

| Displayed binding/menu group | Current behavior and scope |
|---|---|
| Detach, Help, toggle sidebar, workspace picker | The four implemented M2 actions. Navigation inside the picker and mobile Help/Detach menu targets work. |
| User Alt+F popup/custom command | Parsed but direct commands are silently consumed, with no execution or notice. M4 command/popup scope; not a missing core fact. Fixture scripts never send Alt+F or execute its command. |
| Layout/tab navigation and lock | M3a implements indexed workspace/tab/agent selection, previous/next, directional pane focus/swap, cycle/last pane, zoom/resize and Ctrl+G lock with default/configured bindings. |
| Reload config binding/menu | M3a.1 rereads local herdr config, rebuilds bindings/theme/chrome and reallocates panes. Invalid sections keep their old values while valid siblings apply; parse/read failures retain the current config. Manual sidebar width and agent sorting survive. |
| Settings binding/menu | M4 implements the reference overlay: the five sections (theme, indicators, sound, toast, integrations), live theme preview, `write_edit` saves and the same config-reload path as the reload binding. The Integrations section has no `integration.list`/`integration.install` core methods in hc, so it renders the reference's empty state `no integration targets available` and applying it is inert — the reference renders its built-in target list instead, so that one checkpoint differs by the missing core methods (Known differences). |
| Mutation actions | M3a includes new tabs, both split axes, close tab/pane and rename tab/pane, with matching dialogs. M4s1 adds the right-click context menu (workspace/tab/pane targets, reference items/geometry/highlight), workspace new/rename/close including the mobile `+ new workspace` row, and machine dialogs (add/connect/rename/remove). Custom commands, notifications, scrollback editing/clear and remaining actions stay in later M4 slices or M5. |
| Copy mode/selection/clipboard help entries | M5; not an available M2 action. |
| Create/open/remove worktree help entries | Listed for source help fidelity; worktree mutation is outside v1. Read-only Git/worktree presentation remains required. |

The joint review accepted documented deferral of dead-wire held releases,
resize acknowledgement ambiguity, remote clock/timezone and currently unused
token metadata. C3 hostname, Git facts and quiet human release are implemented
and verified in the stated native fixtures. C3 does not supply first-paint appearance; the client cache handles routine switching.
M3b derives completion and recency from authoritative C3 counters. Acknowledgement stays client-local and
only follows a composed, completed visible paint while the client is focused.
There is no cross-connection completion/output fence yet: new inventory can
arrive while stream output is stalled, so a paint can be older than its status.
That stall race remains a documented limitation, not a proven acknowledgement
match. Reporter state labels remain deferred.


## Cross-viewer sizing and passive recovery

The tested core chooses the most recently active non-observe viewer. A phone
attaching to a pane visible on the desktop therefore takes size; desktop
observers follow the authoritative grid. A size holder and the input wire
reconnect with `resumed: true`, without focus or human-input activity. Merely
recovering the desktop transport does not displace the phone. When the phone
detaches, the remaining passive desktop viewers restore the desktop allocation.

Entering or revisiting a tab creates fresh size holders. Moving focus opens
that pane's active input wire, and a real allocation change sends a resize.
Those local activities, input and explicit take-control may reclaim sizing;
unchanged allocation or a mismatching core grid does not trigger a reclaim loop.
Alternating genuine activity can legitimately cause repeated reflows.

This differs from pinned herdr's direct-attach resize lock, which protects the
direct viewer from TUI layout resizes until detach. hc-tui follows hc's current
latest-activity policy and cannot promise that lock. A phone initially wins;
a desktop user returning can take size back. Exact lock parity would require
an explicit core ownership contract, not observer-side guessing. M3a.1's owned
proxy regression cuts only desktop transports, confirms live passive attach
replies plus fresh observer output, verifies phone size retention, automatic
fallback on phone detach and real desktop-allocation reclamation.

## Close identities and the remaining race

In the pinned pre-C3 core, canonical `t...` addresses are not recycled for a
new terminal within preserved core state. The serialized registry retains a
monotonic `next_id`; allocation refuses exhaustion instead of wrapping.
Names may be reassigned, but hc-tui closes canonical addresses. Thus normal
allocation cannot replace a closed record with a different terminal at the
same address between validation and kill. State replacement/reset and remote
origin redirection are outside that per-core identity guarantee.
See the [pinned terminal registry](https://github.com/herdrdev/herdr-core/blob/ec8adcdf0da38e539e8702f7c37801647da035f3/crates/herdr-core/src/terminals.rs#L211).

Displayed herdr-app membership authorizes an explicit user close; adopting the
terminal into an owned layout is not required. The foreign document remains
read-only. A concurrent move after the final membership reread can still race
`terminal.kill`, which has no conditional incarnation/membership guard. Kill
and layout writes are not atomic and group closes may partially succeed. An
absolute conditional-close guarantee remains a core follow-up.

## Cursor-color evidence

Native captures compare cursor position, visibility, shape and blink. M3a.1
also exports and compares any emitted host cursor-color override; the decoder
has an OSC12/set/reset regression. The pinned thin-client cached-switch capture
emits no host cursor-color override on either side (`null` in both captures).
That absence match is not native proof that the program's earlier OSC12 value
survived. Retained program OSC12 state is unit-tested only. Native indexed-cell
colors and OSC4 palette comparisons remain separate evidence. First attach,
reconnect and eviction appearance limitations remain unchanged.


## M2.2 native evidence

Core archive SHA256: `f2ec0f45b87a05dfaf63222f7c80db76231e09f8f68654526d397478ff5512a4`.
Extracted `.8` binary SHA256: `5f6cd86e61fdb90b703fb63a1398cf868c29acb107b0717de47a90c847da4b90`.
The archive was copied, verified and extracted without building or modifying core.
All runs used owned scratch sessions, temporary HOME/XDG roots and read-only
original configuration. Complete cells, attributes, cursor, host modes and titles
are compared; restored frames add one required checkpoint per run.

| Native profile | Matching checkpoints |
|---|---:|
| M2, six sizes | 154 |
| M2 host ModifyOtherKeys profile | 33 |
| Standard repository, six sizes | 18 |
| Explicit hostname tab status and title, 120x40 | 33 |
| Default layout/reload, six sizes | 256 |
| Configured layout/reload, six sizes | 280 |
| Previous-move-only layout | 45 |

Missing/nonexecutable Git and detached HEAD each add two matching checkpoints.
Three promisor cases are excluded from matching totals. All 390 paired immutable
input cases and 14 positioned detach tails match; four opt-in protocol integration
tests pass against `.8`. All 13 layout runs prove two-client convergence and both
reload rounds. M4's Settings overlay replaces the earlier `pending-feature`
notice path: the binding/menu entries now open the real overlay and the layout
scenario compares its cells directly. Original config and expected input bytes
are unchanged, owned resources clean up, and measured disk space stayed above
the 12 GiB floor.

Native validation corrected an automatic worktree grouping assumption: Git
checkout identity and Herdr-managed membership are different facts. It also fixed
one stale three-argument Settings comparison call after the title exemption was
removed. Earlier failed captures are retained outside the repository; they are
not acceptance passes. M3b chronology evidence is recorded separately below;
remote hostname qualification is unit-tested, while this native hostname profile
covers the local endpoint. Appearance, consumption-fence and deferred-action
limits above remain in force.

## M3b chronology evidence

The results below are historical coverage, not acceptance of the correction
round. M3b remains unaccepted. The receipt patch has model/watcher tests;
corrected alias, handoff and full native matrices still require later gates.

The pinned `.8` archive and `d00953e0` reference remain unchanged. Three
agents in named tabs acquire idle readiness, work, then finish in known core
completion order while another tab is visible. Native captures verify Done
priority, repeated-word stability, visit acknowledgement, another-tab attention
and mobile switcher ordering at all six sizes: 114 exact checkpoints. A
WezTerm host profile adds 19 exact checkpoints and proves ModifyOtherKeys1.

The historical restart capture retained the candidate UI across an owned core
restart but relaunched the reference client; it did not prove same-live-reference
UI recovery. The earlier assertion that the reference has no preserving restart
API was incorrect: pinned `server.live_handoff` preserves pane/program processes.
The actual later owned default-local probe replaced the server, then the reference
UI restored the terminal and printed the live-update shutdown error. Its individual
exit status was not saved, and no fresh post-handoff paired frame passed.

Pinned default-local source exits 1 for that shutdown. Candidate R1 still requires
same-live-UI reconnect; this necessary lifecycle distinction does not relax
steady-state local R0 surfaces. Saved-endpoint federation has a separate supported
reconnect path, with a genuinely healthy owned peer and matched topology required
for any paired frames. Its fixture/runtime gate, full-lifecycle reporter retention,
individual exit/liveness evidence and candidate recovery are still pending.
Protocol stdio-adapter evidence cannot establish native SSH acceptance. No client
relaunch is same-live-UI proof. Old captures/classifications remain unchanged.
First reconnect appearance and the stalled-output fence remain unproven.

M2/M2.2/M3a regressions retain their explicit coverage: 154 default, 33 host
MOK, 18 repository, 33 hostname/status and 581 layout checkpoints, plus six
exact Git fallback/detached checkpoints. All 13 layout runs retain shared
layout convergence and both reload rounds. The Settings overlay is ported in
M4 (the former `pending-feature` notice is gone); managed worktree groups
remain `pending-feature`
(M4). Three promisor variants each retain five raw count-cell differences,
`passed: false` and nonzero exit; their full known/unknown control surfaces
explain core policy without masks. They are excluded from matching totals.

All 390 immutable paired input cases and 14 positioned detach tails match;
four opt-in `.8` protocol integrations pass. A fixed100ms tail sample once
captured only nine of eleven reference release packets. The harness now waits
up to five seconds for the unchanged positioned fixture and a stable interval;
missing, extra or malformed bytes still fail. Failed pilots remain excluded.
No expected bytes, original config or foreign structure is rewritten. Owned
sessions clean up and disk stays above the 12 GiB floor. Cross-machine counter
isolation and deterministic merge behavior have model tests, without a native
multi-machine chronology claim.

## Known differences

These are measured behavioral differences, not bugs waived by annotation.
Everything else on the compared surfaces still diffs normally.

- **Chronology after a server restart.** hc keeps its activity domain across
  an owned restart (the epoch changes, existing markers baseline into the new
  epoch). Herdr's `server live-handoff` replaces the server but resets
  chronology to null. A fresh client therefore baselines retained completion
  facts one way in hc-tui and another in herdr, and equal-priority ordering
  after a restart can legitimately differ between the two.
- **Receipt order across machines.** Foreign `(core_id, epoch, scope)`
  domains are never compared with each other's counters; cross-machine agents
  keep their local ingestion order. Two clients watching cores restart or
  report in different sequences can show different equal-priority orders
  without either being wrong.
- **Stalled-output acknowledgement race.** There is no ordered
  output-consumption fence in core: inventory can publish a status while a
  stalled attach stream still holds older bytes, so a paint can be older
  than its status. Client-local acknowledgement follows a composed, fresh,
  focused visible paint; the race itself is a documented core limitation.
- **Reconnect overlay.** After a core restart, retained-pane screens keep
  their last paint while the fresh attach opens, so a brief "Reconnecting /
  Waiting for a fresh terminal paint" notice can appear where the reference
  shows nothing. It is transient — measured clearing in under one second
  after restart in the `.9` native runs — and the chronology checkpoint now
  waits it out and records the delay.
- **Machine context menu and dialogs.** The reference TUI has no machine
  management surface: a right-click on a machine row falls through its
  dispatch and shows nothing. hc-tui opens a machine menu — the Local row
  offers `Add machine`; a remote row offers `Connect`, `Rename machine` and
  `Remove machine`. Add/Rename use the shared rename-dialog surface, Remove
  the shared confirm surface, and Connect needs no dialog. These rows and
  dialogs are an intentional extension, exercised on the fake core only —
  no parity checkpoint compares them and real machines are never added or
  removed in tests.
- **Agent context menu.** The reference TUI has no agent-row menu: a
  right-click on an agent row or chip falls through its dispatch. hc-tui
  opens an extension menu in the reference's style — `Send a line...`
  (a small text dialog → `terminal.send`), `Interrupt`
  (`terminal.interrupt`), `Mark read`/`Mark unread` (`herdr.seen` through
  the dedicated seen writer; unread writes `turn - 1` and is inert at turn
  0), `Keep`/`Park` (`terminal.keep`; park first clears the `herdr.space`/
  `herdr.tab` placement labels like herdr-app) and `Kill` behind the shared
  confirm dialog (`terminal.kill`). Every action re-reads the terminal's
  `created_ms` before acting, like the reference's rename/close flow.
  Exercised on the fake core only — no parity checkpoint compares these
  rows and no real agent is ever sent to, interrupted or killed in tests.
- **Worktree context-menu items.** Managed worktree groups stay
  `pending-feature`: hc-tui workspace facts carry no checkout membership, so
  `is_git`/`is_linked_worktree`/`has_worktree_children` are always false and
  the workspace menu matches the reference's non-Git two-item form. Should a
  Git workspace ever be presented, the worktree items answer `Unavailable`
  rather than mutating anything.
- **Workspace rename persistence.** The reference stores workspace labels in
  its own client state; hc-tui persists the rename as an `adoptions` entry in
  the owned `hc-tui` document (`machine`, `space`, `name`, `custom_label`),
  CAS-written with opaque/concurrent fields preserved. It never writes the
  foreign `herdr-app` document, and owned adoptions override foreign labels
  for cwd-grouped workspaces too.
- **Navigate-mode binding tail.** In the reference's `route_navigate_key`, an
  unmatched key still resolves prefix-dispatch bindings — e.g. `z` executes
  `prefix+z` (Zoom) and leaves Navigate. hc-tui consumes unmatched Navigate
  keys without dispatch. Measured: while Navigate is held open, a plain `z`
  keeps the NAVIGATE bar in hc-tui where the reference exits to Terminal.
  Pane-focus/cycle bindings, the foreign-preview reject notice and the
  custom-command tail of that function are equally undispatched. Pre-dates
  M4s1 (M3a navigate surface); the M4s1 scenario exits Navigate with Escape
  before later checkpoints, and the gap stays open here rather than fixed
  inside this slice.
- **Synthesized notifications.** hc's core emits no notification events; the
  reference receives `SemanticNotification`s from its server. hc-tui derives
  alerts client-side by diffing each pane's effective status word on every
  reconcile — the herdr-app `alert` rule (blocked/error entries and busy
  exits; panes only, never sub-agents; unseen targets only; at most once per
  turn) — then runs the reference client pipeline unchanged (settle delay,
  completion-evidence validation, same-pane dedupe, queue, kind TTL). The
  visible card/banner, OSC 9/99 bytes and `prefix+o` focus are byte/frame
  identical; only the trigger evidence is a word diff instead of a server
  event.
- **Notification sound as a host bell.** The reference plays `done.mp3` /
  `request.mp3` (or configured paths) through an audio player; hc-tui has no
  audio surface and writes a single BEL to the outer terminal per accepted
  sound request. `[ui.sound]` gating, per-agent overrides and the
  `HERDR_DISABLE_SOUND`/`NEXTEST` kill-switches match the reference; the mp3
  path fields parse but are never opened.
- **Terminal bell accounting.** hc's core has no `TerminalBell` message, so
  hc-tui counts BEL bytes inside its VT emulator and forwards them to the
  host in 64-byte chunks. The wire effect is identical to the reference's
  server-counted `ServerMessage::TerminalBell` fanout; the counting point
  moved client-side.
- **`system` delivery falls back to terminal.** hc-tui has no desktop
  notification integration, so `delivery = "system"` writes the same OSC
  9/99 sequence as `terminal`. `off` still synthesizes (and suppresses
  output), `herdr` renders the card/banner.
- **`[ui.toast.clipboard]` and extra kinds.** The reference's copy-toast
  section and the `UpdateInstalled`/`Custom` notification kinds have no
  trigger in hc-tui this slice; the keys are not parsed. `[ui.toast]` itself
  matches the reference, including the legacy `enabled` mapping and the
  3600-second `delay_seconds` cap.
- **`herdr.seen` worker.** The label write and the turn rule are the
  reference's; hc-tui performs them on a dedicated channel worker so a
  failed write retries without touching layout operation results. The wire
  writes are the same `terminal.label` calls.
- **Toast capture timing.** Both sides anchor the same 5/8-second toast
  TTL at delivery, but each parity checkpoint decodes the two owners at
  different instants (the capture loops run one after the other and each
  waits for the pane marker plus host quiet). A live toast therefore
  straddles the pair of capture slots: one frame can hold the card while
  the other has already expired it, or vice versa — a timing artifact, not
  a render difference. The chronology and default profiles consequently
  keep `delivery = "off"` (the reference's default) so every checkpoint is
  deterministic; the card/banner, queue, TTL, suppression and OSC paths
  are covered by the fake-core tests instead.
- **Endpoint config diagnostic merge.** After a Settings save the reference
  pushes `ServerReloadConfig` to its server and merges the endpoint's
  `snapshot.config_diagnostic` into the config toast — identical local and
  endpoint summaries render as `client + endpoint: <summary>`. hc's hello
  has no `server.reload-config` method and no `config_diagnostic` field, so
  hc-tui's reload stays client-local and the toast renders only the local
  `config.toml …; herdr config check` summary. The underlying diagnostics,
  the summary text and the right-aligned yellow banner row are identical;
  the prefix and its extra width are not producible.
- **Integrations section content.** hc advertises no
  `integration.list`/`integration.install` methods, so the reference's
  built-in target list (`pi`, `claude`, … with found/not-found rows and
  install affordances) cannot be produced. hc-tui renders the section's
  reference empty state — `no integration targets available` — and applying
  the row is inert. The `settings-*-integrations` checkpoints therefore
  differ by exactly that section body (measured 209 cells at 80/65 columns,
  211 at 64/40, 1421 at 120/200 — the row count scales with the overlay
  height); every other Settings checkpoint, including the menu-opened path
  at mobile widths, is cell-identical.
- **Navigator footer identity fields.** The "Go to" overlay's selected-row
  footer renders the reference's `detail`/`meta` strings verbatim:
  `workspace / tab / pane_id` and the pane's `cwd`. Two fields carry
  per-run identity that no client-side render logic can unify: hc names
  panes `t<N>` where the reference core names them `w<W>:p<P>`, and each
  parity owner's scratch root is a separate `mkdtemp`, so the `cwd` path
  differs in its random segment (`/tmp/hctui-parity-<seed>/acme-shop`).
  Those cells differ on checkpoints where a pane row (id + path, 12
  cells) or a workspace row (path, 8 cells) is selected; every other
  navigator checkpoint — each filter, search-without-results, movement
  and close — is cell-identical. The row list, search, filters,
  scrolling, mouse handling and focus acceptance are unaffected.

## Slice 5 — popups, peek and custom commands

- **`keys.command` `type = "plugin_action"`.** hc advertises no plugin
  runtime and no `plugin.*` methods, so a plugin keybind cannot invoke
  anything. hc-tui shows the reference's unavailable notice ("Plugin
  actions unavailable") with a hc-specific body explaining that hc has no
  plugin runtime, rather than attempting an RPC.
- **Popup terminal input.** Keyboard input is modal like the reference,
  and Escape or an outside click closes the popup (`terminal.kill`). The
  reference forwards mouse input inside the popup terminal; hc-tui keeps
  the popup modal but does not forward pointer events into its surface —
  clicks inside are ignored.
- **`type = "pane"` zoom.** The reference splits the focused pane
  horizontally and marks the new pane `zoomed` (a full-tab overlay).
  hc-tui performs the same through `Change::Split { vertical: false,
  zoomed: true }` in the layout-document CAS.
- **Peek.** hc has no `terminal.peek` request: the agent-menu Peek item
  opens an observe-only attach on the terminal, drawn as the reference's
  80%×80% floating dialog. Keep performs label → layout CAS →
  `terminal.keep` exactly like `terminal.keep` semantics describe; close
  detaches without writes.
