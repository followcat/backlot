#!/bin/sh
# Builds the pinned herdr-core used by the opt-in real-core suite
# (crates/hc-proto/tests/integration.rs).
#
# Fetches the exact public core commit and its pinned Ghostty submodule
# as source archives, builds with Zig 0.16.0 (official release, sha256
# verified), and writes the binary to target/test-core/herdr-core —
# everything stays under target/, which is never committed. Re-running
# reuses what is already present ONLY while the recorded pins match;
# a pin change wipes sources, toolchains and build output first.
set -eu

CORE=06793ede1bedb1fed8d22d3cae476d3eb239edf0
GHOSTTY=1e5cdb865807abc990181efb8ebfa9122955cce2
ZIG_VERSION=0.16.0
ZIG_SHA256=70e49664a74374b48b51e6f3fdfbf437f6395d42509050588bd49abe52ba3d00
ZIG_TARBALL=zig-x86_64-linux-${ZIG_VERSION}.tar.xz

OUT=target/test-core
SRC=$OUT/src
ZIG_DIR=$OUT/zig
STAMP=$OUT/pins

fetch() {
    # $1 url, $2 destination — bounded retries and timeouts.
    if [ ! -f "$2" ]; then
        curl -fsSL --retry 3 --connect-timeout 15 --max-time 900 "$1" -o "$2.tmp"
        mv "$2.tmp" "$2"
    fi
}

# Pin-keyed caches: any change to CORE/GHOSTTY/ZIG_VERSION discards
# sources, archives, the toolchain and build output before reuse.
if [ "$(cat "$STAMP" 2>/dev/null)" != "$CORE $GHOSTTY $ZIG_VERSION" ]; then
    rm -rf "$SRC" "$ZIG_DIR" "$OUT/build" \
        "$OUT/core.tar.gz" "$OUT/ghostty.tar.gz" "$OUT/$ZIG_TARBALL"
    mkdir -p "$SRC" "$ZIG_DIR"
    echo "$CORE $GHOSTTY $ZIG_VERSION" >"$STAMP"
fi
mkdir -p "$SRC" "$ZIG_DIR"

if [ ! -x "$ZIG_DIR/zig" ]; then
    fetch "https://ziglang.org/download/${ZIG_VERSION}/${ZIG_TARBALL}" "$OUT/$ZIG_TARBALL"
    echo "$ZIG_SHA256  $OUT/$ZIG_TARBALL" | sha256sum -c -
    tar -xJf "$OUT/$ZIG_TARBALL" -C "$ZIG_DIR" --strip-components=1
fi

if [ ! -f "$SRC/Cargo.toml" ]; then
    fetch "https://github.com/herdrdev/herdr-core/archive/${CORE}.tar.gz" "$OUT/core.tar.gz"
    tar -xzf "$OUT/core.tar.gz" -C "$SRC" --strip-components=1
fi

if [ ! -f "$SRC/vendor/ghostty/build.zig" ]; then
    fetch "https://github.com/ghostty-org/ghostty/archive/${GHOSTTY}.tar.gz" "$OUT/ghostty.tar.gz"
    mkdir -p "$SRC/vendor/ghostty"
    tar -xzf "$OUT/ghostty.tar.gz" -C "$SRC/vendor/ghostty" --strip-components=1
fi

# crates/ghostty-vt/build.rs builds libghostty-vt through $ZIG.
ZIG="$PWD/$ZIG_DIR/zig" \
    cargo build --locked --release -p herdr-core \
    --manifest-path "$SRC/Cargo.toml" --target-dir "$OUT/build"
cp "$OUT/build/release/herdr-core" "$OUT/herdr-core"
echo "HC_TUI_TEST_CORE=$PWD/$OUT/herdr-core"
