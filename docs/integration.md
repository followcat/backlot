# Real-core integration tests

`crates/hc-proto/tests/integration.rs` is an **opt-in** suite against a real
`herdr-core` binary. Ordinary `cargo test` ignores it and never discovers a
real `hc`.

## Pins

| Component | Pin |
|---|---|
| herdr-core | `06793ede1bedb1fed8d22d3cae476d3eb239edf0` (`herdrdev/herdr-core`, 0.1.19) |
| Ghostty (`vendor/ghostty`) | `1e5cdb865807abc990181efb8ebfa9122955cce2` |
| Zig | `0.16.0` (`zig-x86_64-linux`, sha256 `70e49664…52ba3d00`) |

`tools/build-test-core.sh` fetches the exact source archives, verifies the
official Zig tarball checksum, builds, and prints the binary path. All
outputs stay under `target/` and are never committed.

## Run

```sh
tools/build-test-core.sh                       # once; produces the binary
HC_TUI_TEST_CORE=$PWD/target/test-core/herdr-core \
    cargo test -p hc-proto --test integration -- --ignored --test-threads=1
```

`crates/hc-tui/tests/real_core_smoke.rs` drives the real `hc-tui` binary in
a PTY over the same scratch isolation — idle start, sidebar rows, attach and
echo, pane cycling, tab and split creation, an OSC 7501 report, and a clean
detach with restored terminal modes. It asserts only facts both cores
share, so the same suite covers fork and official builds:

```sh
HC_TUI_TEST_CORE=$(command -v hc) \
    cargo test -p hc-tui --test real_core_smoke -- --ignored --test-threads=1
```

A missing `HC_TUI_TEST_CORE` fails the explicit run — it never silently
counts as skipped. `--test-threads=1` keeps the real cores serial; each
test could run in parallel but serial runs are easier to debug. A second
pinned build (for fork-compat checks) is run the same way by pointing the
variable at it — the suite itself is identical.

## Scratch isolation

Every test owns a fresh `ScratchCore` (`tests/common/scratch.rs`):

- session `hctui-test-<random>` with `HC_SESSION` + explicit short `HC_SOCKET`;
- temporary `HOME`, `XDG_STATE_HOME`, `XDG_CONFIG_HOME`, `XDG_RUNTIME_DIR`
  under one `/tmp/hctui-*` root; `env_clear()` — no inherited `HC_*` leaks in;
- `core.json` pre-seeded with the neutral identity `host-a`;
- a foreground `serve` in its own process group (never an existing service);
- logs under the owned root; finite deadlines on every step.

Teardown runs `hc --json server stop` under the identical fixture env and
requires `stopped:true` — the only path that stops the detached holders
and their programs (they `setsid` away and survive a killed core; no
signal to the serve reaches them). After a deliberate mid-test kill or a
failed stop, a replacement `serve` adopts the holders, then stops
normally. Every wait is bounded; on verified failure the root is
preserved and reported instead of removed.
