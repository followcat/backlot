# Herdr parity

The acceptance target is **herdr 0.9.5-followcat**, source revision
`d00953e081389eb5f8ea006896fbf4a17b2f9dfc`. With the same configuration,
terminal size, programs and states, matching surfaces must produce the same
cells, glyphs, colors and attributes, and react the same way to keys and
mouse input.

**Validation status:** M2 provides the read-only shell surfaces below. The
owned-session harness checks native cells, cursor, host modes, window titles
and actual child input against the pinned executable. Final results and
remaining review questions are recorded in the M2 progress entry and private
joint review. This does not declare the later milestones implemented.

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
| Shared completion acknowledgement | Core has advisory `herdr.seen` labels but no equivalent native shared per-status acknowledgement state. M2 applies source transitions in a local presentation cache, scoped to turn/incarnation, without label writes. Acknowledgement is not synchronized or persisted across hc-tui clients. |
| Global state-change ordinal | `status_ms`, terminal `turn` and observed publishes are available; the source's complete pre-connection global history is not. Use timestamps and local observed transitions, with deterministic ties. Comparing turn magnitudes between terminals is not a replacement for chronology. |
| Workspace branch | No documented equivalent in the tested core terminal inventory. Leave the source optional branch field absent; do not probe local repositories for remote facts. |
| Git ahead/behind counts | No documented equivalent. Leave the source optional counts absent. |
| Worktree identity and label | No documented equivalent to the source workspace worktree object. Leave it absent. |
| Workspace/agent tokens | No documented mapping for the source configurable token/context-usage values. Leave token collections empty. Unknown protocol extensions are retained without guessing their meaning. |
| Agent state labels | No documented equivalent for the source server's state-label collection. Leave it empty; core parent addresses or record sources are not state labels. |
| Worker relationship | Core `parent` is available and remains verbatim. The reference sidebar has no parent-note field; a worker uses an ordinary agent row with its own identity. Unknown parents do not hide it. |
| Endpoint state | Local connection/subscription state and remote `machine.list` states drive the reference endpoint enum. Offline snapshots remain listed. RPC-only loss retains streaming terminal reports and marks stale machine information; subscription loss makes the retained inventory stale. Use the source stale/dimming presentation. |
| Size-owner identity | Active/observe role, size and focus facts are available, but the tested protocol does not name another size owner. Preserve role and authoritative sizes, and omit an invented owner name. Visible ownership notices and explicit take-control behavior still require validation. |
| Hostname | Core inventory exposes the configurable `core.name`, not a separate OS hostname. Use that name for hostname status/title tokens; do not run extra local or remote commands to discover it. |
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

In both tested core revisions, a human key-release DATA frame counts as
activity and can reclaim size, unlike the source's quiet release handling.
Two owned viewers demonstrated the exact case: a later viewer held 88x22;
the original 66x16 viewer sent a real key release and core returned to 66x16.
`resumed` suppresses recovery-attach activity, not subsequent DATA activity.

M2 sends the actual release to its original wire. It does not relabel human
releases as AUTOMATIC_INPUT, fake focus or resize the other viewer to hide
this core behavior. The available generation-1 controls have no quiet-human-
release flag. This exception is limited to the resulting activity/ownership
change; encoded bytes, original-target routing and ordinary focus/resize
behavior must remain correct.


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
cell's resolved underline color. The default title fixture permits exactly
one data adaptation: when the template is `{hostname}: {workspace}`, the
reference's OS-hostname prefix may differ from the fixture core's `host-a`
prefix, with an identical workspace suffix. It records this difference
explicitly. Other title templates compare exactly; no cells are masked.

Detach must restore raw mode, cursor visibility, line wrap, keyboard flags,
alternate screen, paste, focus, mouse and scheme reporting. The tested
reference retains its last window title on exit, so hc-tui follows that
measured behavior rather than inventing a title reset.
