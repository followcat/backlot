#!/bin/sh
# Installs the pinned Zig toolchain under DIR (argument 1) from the
# official ziglang.org release, sha256-verified before extraction.
# The archive matches the host platform unless TRIPLE (argument 2)
# names another supported release archive, e.g. "aarch64-macos".
# Honours the caller's proxy environment. On success DIR/zig is the
# compiler; used by the ci jobs (see .github/workflows/).
set -eu

ZIG_VERSION=0.16.0

DIR=${1:?usage: install-zig.sh DIR [TRIPLE]}
TRIPLE=${2:-}

if [ -z "$TRIPLE" ]; then
    case "$(uname -s)" in
        Linux) os=linux ;;
        Darwin) os=macos ;;
        *) echo "install-zig.sh: unsupported host OS" >&2; exit 1 ;;
    esac
    case "$(uname -m)" in
        arm64) arch=aarch64 ;; # macOS names aarch64 "arm64"
        *) arch=$(uname -m) ;;
    esac
    TRIPLE="$arch-$os"
fi

# sha256 of each official ziglang.org 0.16.0 release archive, per the
# published download index (ziglang.org/download/index.json).
case "$TRIPLE" in
    x86_64-linux) ZIG_SHA256=70e49664a74374b48b51e6f3fdfbf437f6395d42509050588bd49abe52ba3d00 ;;
    aarch64-linux) ZIG_SHA256=ea4b09bfb22ec6f6c6ceac57ab63efb6b46e17ab08d21f69f3a48b38e1534f17 ;;
    x86_64-macos) ZIG_SHA256=0387557ed1877bc6a2e1802c8391953baddba76081876301c522f52977b52ba7 ;;
    aarch64-macos) ZIG_SHA256=b23d70deaa879b5c2d486ed3316f7eaa53e84acf6fc9cc747de152450d401489 ;;
    *) echo "install-zig.sh: unsupported archive '$TRIPLE'" >&2; exit 1 ;;
esac
ZIG_TARBALL=zig-${TRIPLE}-${ZIG_VERSION}.tar.xz

# sha256 of one file: sha256sum (GNU), shasum (BSD/macOS) or openssl.
file_sha256() {
    if command -v sha256sum >/dev/null 2>&1; then
        sha256sum "$1"
    elif command -v shasum >/dev/null 2>&1; then
        shasum -a 256 "$1"
    elif command -v openssl >/dev/null 2>&1; then
        openssl dgst -sha256 -r "$1"
    else
        echo "install-zig.sh: no sha256 tool found" >&2
        return 1
    fi | awk '{print $1}'
}

mkdir -p "$DIR"

# Bounded download: retries cover connection resets and TLS errors.
curl -fsSL --retry 3 --retry-all-errors --connect-timeout 15 --max-time 900 \
    "https://ziglang.org/download/${ZIG_VERSION}/${ZIG_TARBALL}" \
    -o "$DIR/$ZIG_TARBALL"
[ "$(file_sha256 "$DIR/$ZIG_TARBALL")" = "$ZIG_SHA256" ] || {
    echo "install-zig.sh: sha256 mismatch for $ZIG_TARBALL" >&2
    exit 1
}
tar -xJf "$DIR/$ZIG_TARBALL" -C "$DIR" --strip-components=1
rm "$DIR/$ZIG_TARBALL"

[ "$("$DIR/zig" version)" = "$ZIG_VERSION" ]
