#!/bin/sh
# Installs the pinned Zig toolchain under DIR (argument 1) from the
# official ziglang.org release, sha256-verified before extraction.
# Honours the caller's proxy environment. On success DIR/zig is the
# compiler; used by the ci checks job (see .github/workflows/ci.yml).
set -eu

ZIG_VERSION=0.16.0
ZIG_SHA256=70e49664a74374b48b51e6f3fdfbf437f6395d42509050588bd49abe52ba3d00
ZIG_TARBALL=zig-x86_64-linux-${ZIG_VERSION}.tar.xz

DIR=${1:?usage: install-zig.sh DIR}
mkdir -p "$DIR"

# Bounded download: retries cover connection resets and TLS errors.
curl -fsSL --retry 3 --retry-all-errors --connect-timeout 15 --max-time 900 \
    "https://ziglang.org/download/${ZIG_VERSION}/${ZIG_TARBALL}" \
    -o "$DIR/$ZIG_TARBALL"
echo "$ZIG_SHA256  $DIR/$ZIG_TARBALL" | sha256sum -c -
tar -xJf "$DIR/$ZIG_TARBALL" -C "$DIR" --strip-components=1
rm "$DIR/$ZIG_TARBALL"

[ "$("$DIR/zig" version)" = "$ZIG_VERSION" ]
