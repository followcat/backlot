#!/bin/sh
# hc-tui installer: picks the release archive for this platform,
# verifies it against the release's sha256sums.txt, and installs the
# binary — nothing is written to the install dir before the checksum
# matches.
#
#   sh install.sh                 # latest release
#   HC_TUI_VERSION=v0.2.0 sh install.sh
#   HC_TUI_INSTALL_DIR=~/bin sh install.sh
#
# HC_TUI_REPO (default followcat/hc-tui) and HC_TUI_BASE_URL
# (default the repo's GitHub releases URL) exist for mirrors and
# tests — the public install path is always HTTPS.
set -eu

REPO=${HC_TUI_REPO:-followcat/hc-tui}
BASE_URL=${HC_TUI_BASE_URL:-https://github.com/$REPO/releases}
INSTALL_DIR=${HC_TUI_INSTALL_DIR:-${HOME:?set HOME or HC_TUI_INSTALL_DIR}/.local/bin}

die() {
    echo "install.sh: $*" >&2
    exit 1
}

for tool in curl tar awk mktemp; do
    command -v "$tool" >/dev/null 2>&1 || die "needs '$tool' on PATH"
done

# sha256 of one file: sha256sum (GNU), shasum (BSD/macOS) or openssl.
file_sha256() {
    if command -v sha256sum >/dev/null 2>&1; then
        sha256sum "$1"
    elif command -v shasum >/dev/null 2>&1; then
        shasum -a 256 "$1"
    elif command -v openssl >/dev/null 2>&1; then
        openssl dgst -sha256 -r "$1"
    else
        die "no sha256 tool found (need sha256sum, shasum or openssl)"
    fi | awk '{print $1}'
}

# --- platform -------------------------------------------------------------
sys=$(uname -s)
mach=$(uname -m)
unsupported() {
    die "no prebuilt hc-tui for $sys/$mach — releases cover Linux \
x86_64/aarch64 (static musl) and macOS arm64/x86_64; build from source: \
https://github.com/$REPO#build-and-run"
}
case "$sys" in
    Linux)
        case "$mach" in
            x86_64 | amd64) target=x86_64-unknown-linux-musl ;;
            aarch64 | arm64) target=aarch64-unknown-linux-musl ;;
            *) unsupported ;;
        esac
        ;;
    Darwin)
        case "$mach" in
            arm64 | aarch64) target=aarch64-apple-darwin ;;
            x86_64) target=x86_64-apple-darwin ;;
            *) unsupported ;;
        esac
        ;;
    *) unsupported ;;
esac

# --- version --------------------------------------------------------------
if [ "${HC_TUI_VERSION:-}" ]; then
    case "$HC_TUI_VERSION" in
        v*) tag=$HC_TUI_VERSION ;;
        *) tag="v$HC_TUI_VERSION" ;;
    esac
else
    # releases/latest redirects to releases/tag/<tag> — follow it with a
    # HEAD request and take the last path component as the tag.
    url=$(curl -fsSIL -o /dev/null -w '%{url_effective}' "$BASE_URL/latest") \
        || die "could not resolve the latest release"
    tag=${url##*/}
    case "$tag" in
        v*) ;;
        *) die "could not determine the latest release tag from '$url'" ;;
    esac
fi
# The tag becomes a URL path component — reject anything suspicious.
case "$tag" in
    *[!A-Za-z0-9._-]*) die "malformed version '$tag'" ;;
esac
version=${tag#v}

# --- download + verify -----------------------------------------------------
archive="hc-tui-$version-$target.tar.gz"
work=$(mktemp -d)
tmp_dest=
cleanup() {
    rm -rf "$work"
    [ -z "$tmp_dest" ] || rm -f "$tmp_dest"
}
trap cleanup EXIT
trap 'exit 1' INT TERM

# No --retry-all-errors: it needs curl 7.71+, older releases reject it.
curl -fsSL --retry 3 --connect-timeout 15 --max-time 600 \
    "$BASE_URL/download/$tag/$archive" -o "$work/$archive" \
    || die "download failed — does release $tag contain $archive?"
curl -fsSL --retry 3 --connect-timeout 15 --max-time 120 \
    "$BASE_URL/download/$tag/sha256sums.txt" -o "$work/sha256sums.txt" \
    || die "download failed — release $tag has no sha256sums.txt"

expected=$(awk -v a="$archive" '{n = $2; sub(/^\*/, "", n); if (n == a) print $1}' \
    "$work/sha256sums.txt")
[ -n "$expected" ] || die "$archive is not listed in sha256sums.txt"
actual=$(file_sha256 "$work/$archive")
[ "$actual" = "$expected" ] \
    || die "checksum mismatch for $archive — nothing was installed"

tar -xzf "$work/$archive" -C "$work"
[ -f "$work/hc-tui" ] || die "$archive did not contain an hc-tui binary"

# --- install ---------------------------------------------------------------
mkdir -p "$INSTALL_DIR"
# Write inside the destination directory, then rename: the new binary
# appears atomically, never a half-written hc-tui.
tmp_dest="$INSTALL_DIR/.hc-tui.tmp.$$"
cp "$work/hc-tui" "$tmp_dest"
chmod 755 "$tmp_dest"
mv -f "$tmp_dest" "$INSTALL_DIR/hc-tui"
tmp_dest=

echo "installed hc-tui $version ($target) to $INSTALL_DIR/hc-tui"
case ":$PATH:" in
    *":$INSTALL_DIR:"*) ;;
    *) echo "note: $INSTALL_DIR is not on your PATH — add it or run the binary by its full path" ;;
esac
echo "hc-tui needs the hc core installed: https://github.com/herdrdev/herdr-core"
