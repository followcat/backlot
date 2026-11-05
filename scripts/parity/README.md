# Owned-session parity harness

The reference is herdr 0.9.5-followcat at
`d00953e081389eb5f8ea006896fbf4a17b2f9dfc`. Build that revision in a separate
checkout. Supply a JSON manifest containing `source_revision`, the absolute
`binary` path, and its SHA-256 in `sha256`. The harness verifies the hash;
keep this manifest and captures outside this repository.

Build the candidate and shared native cell decoder:

```sh
cargo build -p hc-tui --bin hc-tui --example parity_cells
python3 scripts/parity/run.py \
  --hc /path/to/pinned/hc \
  --hc-tui target/debug/hc-tui \
  --decoder target/debug/examples/parity_cells \
  --reference-manifest /path/to/reference-build.json \
  --config /path/to/herdr/config.toml \
  --out /path/outside/repository/screen-run
python3 scripts/parity/vectors.py \
  --hc /path/to/pinned/hc \
  --decoder target/debug/examples/parity_cells \
  --hc-tui target/debug/hc-tui \
  --config /path/to/herdr/config.toml \
  --reference-manifest /path/to/reference-build.json \
  --out /path/outside/repository/input-run
```

Requires Linux, Python 3.11+, and the Rust/Zig build dependencies. No Python
packages are needed. `--size` selects one of the three required dimensions;
without it the screen runner uses 80x24, 120x40, 200x50, 65x24,
64x24 and 40x24. `--smoke` checks
only the baseline and is not an acceptance run. `vectors.py --fixture`
selects an immutable reference capture; the default runs every capture.

Each client gets a unique explicit scratch session, cleared inherited
environment and temporary HOME/XDG roots. Neutral alpha/beta programs expose
Unicode, styled cells, history and input logs. Both PTYs receive matching
host replies and key/mouse scripts. Reference status reports and matching
core OSC reports are authoritative fixture actions, never screen detection.

The original configuration is read-only and hash-checked after teardown.
Only its private copies disable update checks and sound. Each environment
sets `HERDR_DISABLE_SOUND=1` and an owned `TMPDIR`. Startup/connection
tables, automatic status commands and custom command bindings are rejected,
except the specifically audited inert Alt+F binding, which scripts never
send. Command text is never printed or executed. The mouse byte fixtures additionally set only
their recorded neutral mouse modifier/scroll settings on the private copy.

Screen checkpoints include prefix/help/filtering, navigation, switching,
sidebar dragging/collapse, live cursor/mouse modes, status transitions,
history scrolling, typing-to-bottom and four successive resize events.
The decoder feeds the complete pre-detach byte stream through the same
pinned native VT, replaying resize offsets in order. It compares every cell's
grapheme, wide metadata, source/resolved colors (including underline),
attributes, cursor, host modes and window title. Checkpoints wait for their
expected content and a quiet interval. Every title compares exactly, including
the OS hostname; there is no core-name adaptation. An
unwritten narrow blank is canonicalized to an ASCII space on both sides;
styles and wide tails are retained. There are no region masks or byte
stripping. Teardown output is captured separately and decoded to verify
host-mode resets and retained-title parity, as well as termios restoration.

The input runner compares real program bytes with the committed expected
bytes captured from the reference: 100 basic keys, 23 held/layout/live-mode/
orphan cases, seven focus cases, 35 functional-key cases and 225 mouse cases
(cell, pixel and unknown pitch): 390 cases. Supplying the reference manifest
replays them through both executables and checks their detach tails too.
Only explicit source-derived `detach_tail` drain segments permit unordered
CSI-u releases, because the source drains a HashMap. Their exact contents,
duplicates and positions relative to focus/other bytes remain significant.
Late ordinary input before detach fails separately; absent drain metadata
means exact comparison. Ordinary cases compare exact byte order. It must never
regenerate expected bytes from the candidate.

Each result records binary/config hashes, checkpoint diffs, terminal-mode
restoration and verified cleanup. A failed cleanup retains a private
recovery record and scratch root instead of claiming success. Do not commit
captures, logs, config copies, credentials or recovery files. A passing run
covers its explicit fixtures; broader configuration and unavailable core
facts remain subject to [the parity contract](../../docs/parity.md).


M2.1 tightens readiness: help paging must change visible content, requested
status glyph/color must reach its named row, live cursor/mouse modes must be
active, and resize must reach the actual decoded grid. Status readiness uses
the pinned source default-dark Catppuccin/dots oracle; other configured
palettes are rejected until an independent source oracle is supplied.
The decoder additionally records modes9/1000/1002/1005/1015 and outer
ModifyOtherKeys0/1/2; teardown requires the emulated host default Block/steady
cursor as well as visibility. Decoder tests exercise each added mode and a
live Bar/blink→default restore, including control-string-safe MOK tracking.


The 65/64 boundary and 40-column runs use the narrow script, with mobile
switcher status/highlight and normal-mode readiness plus menu-to-Help input.
`--host-program wezterm` adds a host ModifyOtherKeys1 profile; capture and
teardown must prove it is enabled and then reset.

Use `--scenario repository` with the pinned C3 `.8` binary for the Git fixture
(default120x40, with explicit `--size` also supporting65/64/40). The standard
case creates ↑2 ↓1 versus a local upstream, a linked worktree and an unlabelled
subdirectory terminal. Core facts must settle and pass field assertions before
its frontend starts. `--repository-case` also accepts `missing-git`,
`nonexecutable-git`, `detached`, `promisor`, `promisor-false` and `promisor-empty`.
Only owned repositories and environments change; no fetch occurs.

Promisor counts remain absent by core policy. Full raw differences still fail;
a policy annotation never excuses an unrelated screen/cursor/mode/title error.
An exact surface with a semantic policy annotation returns exit3 and
`passed:false`, never a parity pass. Complete post-detach frames are required
checkpoints. Any exception or cleanup failure clears `passed`.

The ownership runner captures the exact negotiated kind7 payload during the
human release interval, requires matching child bytes, and verifies the phone
keeps its allocation. Missing capability confirmation uses DATA in protocol
fake-core tests. Native `.8` results remain pending until its release is pinned.

Initial fixture readiness also decodes the complete native screen: ANSI
cursor/style writes can split a visible string, so raw substring matching
is not an adequate readiness condition. The input runner requires the same
`--decoder` as the screen runner.


`--scenario layout` exercises M3a against the same owned-session and binary
provenance checks. It reads an immutable foreign structure fixture, splits
right/down, focuses/swaps/resizes/zooms panes, opens rename/new-tab/close dialogs,
drags tabs and split seams, checks lock passthrough, and runs two hc-tui
processes against one scratch core. The two real clients must commit both
concurrent rename intentions and reach identical cells. Fake-core tests force
the CAS conflict interleaving deterministically.

Child programs report actual PTY sizes. Mutation readiness requires a newer
hc-tui document and changed reference layout/tab state; sizes are checked
before and after screen capture. Every UI-only step checks exact unchanged
program input, while the locked prefix step expects exactly its sent bytes.
A program sets a bar cursor, OSC 12 and OSC 4 once, then a cached return is
captured without repainting the program. Unit tests also inspect retained
OSC 12 state directly. This covers cached switching; first attach, reconnect
and eviction remain the documented limitations. The scenario runs at the
same desktop and mobile sizes; desktop-only tab dragging has no mobile tab
bar counterpart. Captures and configuration copies remain outside the repo.


M3a.1 extends `--scenario layout` with two live config reload rounds. Both owned
copies are edited identically, preserving sound/update guards and inert command
bindings. The source must visibly change theme; new prefixes/Help bindings and
pane allocation must match. At64/40 columns the second reload uses the actual
mobile menu. Settings is exercised from its binding and mobile menu: the
candidate notice is required and expires, while every source-overlay difference
is retained as `pending-feature`, outside matching-checkpoint totals.

The native decoder also records any emitted host cursor-color override. Pinned
cached-switch captures report no override on either side; retained program OSC12
is still unit-only evidence, not established by that absence match.

`ownership.py` accepts the same hc/hc-tui/config/decoder/out arguments. It creates
one scratch core, an owned Unix proxy for the desktop and a separate40x20
non-observe hc attach as the phone. It cuts only proxy transports, requires
successful live resumed reattachments and fresh observer output, checks the
phone keeps size, then checks automatic desktop fallback and real allocation
reclamation. It never discovers or kills an existing session. Its result is a
cross-viewer policy test, not a claim of herdr direct-attach-lock parity.
