#!/bin/sh
# Cross linker for Rust musl builds: drives `zig cc` at the Zig target
# named by HC_TUI_LINK_TARGET. The system cc cannot link a foreign
# architecture (e.g. aarch64 on an x86_64 host); zig carries every musl
# variant itself. Used by .github/workflows/release.yml through
# CARGO_TARGET_<TRIPLE>_LINKER.
set -eu
exec "${ZIG:-zig}" cc -target "${HC_TUI_LINK_TARGET:?set the zig target}" "$@"
