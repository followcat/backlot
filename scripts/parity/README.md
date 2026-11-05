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
  --hc-tui target/debug/hc-tui \
  --config /path/to/herdr/config.toml \
  --reference-manifest /path/to/reference-build.json \
  --out /path/outside/repository/input-run
```

Requires Linux, Python 3.11+, and the Rust/Zig build dependencies. No Python
packages are needed. `--size` selects one of the three required dimensions;
without it the screen runner uses 80x24, 120x40 and 200x50. `--smoke` checks
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
expected content and a quiet interval. The default hostname title has only
the exact documented core-name adaptation; custom titles compare exactly. An
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
