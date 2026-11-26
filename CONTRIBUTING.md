# Contributing to backlot

Read [AGENTS.md](AGENTS.md) and [docs/studio/testing.md](docs/studio/testing.md) before making changes.

Run the workspace gates before committing:

```sh
cargo fmt --check
cargo clippy --all-targets -- -D warnings
sh scripts/check-test-tmp.sh
```

## macOS differences

Keep every platform difference covered so Linux runs catch what macOS CI hits:

- **`TMPDIR` is a link:** `/var` resolves to `/private/var` on macOS, so compare canonical/real paths (`test_support::same_path` / `assert_same_path` in stage tests) rather than raw stringified temp paths. `scripts/check-test-tmp.sh` now defaults to a symlinked `TMPDIR` on Linux too (`BACKLOT_TEST_TMP_NOLINK=1` opts out).
- **Never rely on `read_dir` order:** macOS APFS and Linux filesystems yield directory entries in different orders; sort explicitly whenever output or priority depends on traversal order.
- **The filesystem is case-insensitive:** APFS defaults to case-insensitive names, so never rely on files or fixture names differing only by ASCII case.
- **No wall-clock assertions:** inject deterministic clocks (`App::set_tab_status_clock`, `App::use_tokio_clock`) or pause Tokio time (`#[tokio::test(start_paused = true)]`) rather than sleeping or asserting tight wall-clock bounds.
