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
| `done`, `ready`, `finished` | Completion presentation, with the source acknowledgement behavior |
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
| Global state-change ordinal | Optional C3 `activity.records[""].state_ordinal` and `completion_ordinal` are retained with `core_id`, `epoch`, `scope`, `revision` and named records. Counters are origin-local, never a global order across machines. Chronology/completion inference and attention ordering remain M3b; M2.2 does not sort by these counters. |
| Workspace branch | C3 `repository.branch`, displayed only for `state == "ready"`. Pending/stale/unavailable and future states retain their full facts but hide Git presentation; absence means unsupported. No viewer Git probe. Validated against the pinned `.8` fixtures. |
| Git ahead/behind counts | Nullable C3 `repository.ahead_behind`, with checked u64-to-usize conversion. Unknown stays absent, distinct from observed `(0,0)`. Promisor configuration, including false/empty values, deliberately suppresses counts. Source policy differences are recorded separately, never passes. Validated against the pinned `.8` fixtures. |
| Worktree identity and label | Logical C3 identity groups by `(repository.core_id, checkout.repository_key)` and distinguishes `checkout_key`; all keys/paths stay opaque. Ready facts retain names, root, linked and bare flags in neutral checkout metadata. Ordinary linked Git checkouts remain independent workspace/branch rows, matching herdr. Herdr visual groups require explicit `worktree_space` membership, which C3 does not supply; managed-group parity is unproven and is not inferred from a Git linked flag. |
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

The source's server process boot identity and per-status completion/seen
map do not exist in the tested core inventory. Core `id` is persistent
across a transport reconnect, so reconnect alone must not reset the local
acknowledgement cache. M2 keys that cache by core identity, terminal address
and `created_ms`, then follows observed work/completion transitions and turn
changes. Initial idle reports use the source's acknowledged baseline.

Root `status_ms` seeds initial ordering. Named records have no equivalent
complete global change ordinal. After connection, local observed changes
supply a monotonic chronology; metadata-only updates do not advance it.
Simultaneous changes use deterministic ties. This does not reconstruct
unobserved history or synchronize acknowledgement between clients.

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
| Settings binding/menu | `pending-feature`: the M4 overlay is deferred. M3a.1 shows the transient notice `Settings isn't available in hc-tui yet`. Menu presentation remains identical; the notice/overlay difference is retained, never counted as matching Settings UI. |
| Mutation actions | M3a includes new tabs, both split axes, close tab/pane and rename tab/pane, with matching dialogs. Workspace/machine actions, custom commands, notifications, scrollback editing/clear and remaining actions stay in M4 or M5. Mobile new-tab is active; new-workspace remains deferred. |
| Copy mode/selection/clipboard help entries | M5; not an available M2 action. |
| Create/open/remove worktree help entries | Listed for source help fidelity; worktree mutation is outside v1. Read-only Git/worktree presentation remains required. |

The joint review accepted documented deferral of dead-wire held releases,
resize acknowledgement ambiguity, remote clock/timezone and currently unused
token metadata. C3 hostname, Git facts and quiet human release are implemented
and verified in the stated native fixtures. C3 does not supply first-paint appearance; the client cache handles routine switching.
Completion chronology remains M3b; C3 counters are retained for that slice. Acknowledgement stays client-local and
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
reload rounds. The 17 Settings notice checks retain `pending-feature`; actual
Settings overlay parity is not claimed. Original config and expected input bytes
are unchanged, owned resources clean up, and measured disk space stayed above
the 12 GiB floor.

Native validation corrected an automatic worktree grouping assumption: Git
checkout identity and Herdr-managed membership are different facts. It also fixed
one stale three-argument Settings comparison call after the title exemption was
removed. Earlier failed captures are retained outside the repository; they are
not acceptance passes. Chronology/attention and completion inference remain M3b;
remote hostname qualification is unit-tested, while this native hostname profile
covers the local endpoint. Appearance, consumption-fence and deferred-action
limits above remain in force.
