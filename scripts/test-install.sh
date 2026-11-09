#!/bin/sh
# Offline tests for scripts/install.sh. A file:// fixture directory
# plays the GitHub release, PATH-stubbed uname/curl play the platform
# and the /latest redirect — no network is touched. Run: sh
# scripts/test-install.sh
set -u

ROOT=$(CDPATH= cd -- "$(dirname -- "$0")/.." && pwd)
INSTALLER=$ROOT/scripts/install.sh
SYSBIN=/usr/bin:/bin

work=$(mktemp -d)
trap 'rm -rf "$work"' EXIT
trap 'exit 1' INT TERM
pass=0
fail=0
ok() { pass=$((pass + 1)); echo "ok - $1"; }
bad() {
    fail=$((fail + 1))
    echo "FAIL - $1" >&2
}

# sha256 of one file, matching install.sh's tool fallback.
file_sha256() {
    if command -v sha256sum >/dev/null 2>&1; then
        sha256sum "$1"
    elif command -v shasum >/dev/null 2>&1; then
        shasum -a 256 "$1"
    else
        openssl dgst -sha256 -r "$1"
    fi | awk '{print $1}'
}

# --- fixture ---------------------------------------------------------------
FIX=$work/releases
# make_release TAG VERSION TARGETS... — one tar.gz per target (a fake
# binary whose content names its build) plus sha256sums.txt.
make_release() {
    tag=$1
    version=$2
    shift 2
    dir=$FIX/download/$tag
    mkdir -p "$dir"
    sums=
    for target in "$@"; do
        staging=$work/stage-$version-$target
        mkdir -p "$staging"
        printf 'fake-hc-tui %s %s\n' "$version" "$target" >"$staging/hc-tui"
        chmod 755 "$staging/hc-tui"
        : >"$staging/LICENSE"
        : >"$staging/NOTICE"
        echo readme >"$staging/README.md"
        tar -czf "$dir/hc-tui-$version-$target.tar.gz" -C "$staging" .
        sums="$sums$(file_sha256 "$dir/hc-tui-$version-$target.tar.gz")  hc-tui-$version-$target.tar.gz
"
    done
    printf '%s' "$sums" >"$dir/sha256sums.txt"
}

TARGETS="x86_64-unknown-linux-musl aarch64-unknown-linux-musl aarch64-apple-darwin x86_64-apple-darwin"
# shellcheck disable=SC2086
make_release v0.1.0 0.1.0 $TARGETS
make_release v9.9.9 9.9.9 x86_64-unknown-linux-musl

# The fixture core installers: one drops an hc next to hc-tui (the test
# env passes HC_TUI_INSTALL_DIR through to the installer's children), the
# other fails — neither touches the network.
cat >"$FIX/core-install.sh" <<'EOF'
#!/bin/sh
echo fake-hc >"$HC_TUI_INSTALL_DIR/hc"
chmod 755 "$HC_TUI_INSTALL_DIR/hc"
EOF
cat >"$FIX/core-fail.sh" <<'EOF'
#!/bin/sh
echo "core boom" >&2
exit 3
EOF

# --- tool stubs -------------------------------------------------------------
BIN=$work/bin
mkdir -p "$BIN"
cat >"$BIN/uname" <<EOF
#!/bin/sh
case "\${1:-}" in
    -s) echo "\${FAKE_SYS:?FAKE_SYS unset}" ;;
    -m) echo "\${FAKE_MACH:?FAKE_MACH unset}" ;;
    *) exec $(command -v uname) "\$@" ;;
esac
EOF
cat >"$BIN/curl" <<EOF
#!/bin/sh
# Behaves like real curl on the /latest redirect: url_effective is the
# target only when -L (alone or bundled like -fsSIL) is among the args.
last=
follow=
for a in "\$@"; do
    last=\$a
    case "\$a" in -*L* | --location*) follow=1 ;; esac
done
case "\$last" in
    */latest)
        if [ "\$follow" ]; then
            printf '%s\n' "\${HC_TUI_FAKE_REDIRECT:?redirect unset}"
        else
            printf '%s\n' "\$last"
        fi
        ;;
    *) exec $(command -v curl) "\$@" ;;
esac
EOF
chmod 755 "$BIN/uname" "$BIN/curl"

# run DEST SYS MACH [VAR=val ...] — the installer under the fixture env.
run() {
    dest=$1
    sys=$2
    mach=$3
    shift 3
    env -i PATH="$BIN:$SYSBIN" HOME="$work/home" \
        FAKE_SYS=$sys FAKE_MACH=$mach \
        HC_TUI_BASE_URL="file://$FIX" HC_TUI_INSTALL_DIR="$work/$dest" \
        HC_TUI_CORE_URL="file://$FIX/core-install.sh" \
        "$@" sh "$INSTALLER" 2>&1
}

want() { # name haystack needle
    case "$2" in
        *"$3"*) ok "$1" ;;
        *) bad "$1 — expected '$3' in: $2" ;;
    esac
}

# --- per-platform archive choice --------------------------------------------
n=0
check_platform() { # sys mach target
    n=$((n + 1))
    dest="dest-$n"
    out=$(run "$dest" "$1" "$2" HC_TUI_VERSION=v0.1.0)
    st=$?
    [ $st -eq 0 ] || {
        bad "$1/$2 install — exit $st: $out"
        return
    }
    got=$(cat "$work/$dest/hc-tui" 2>/dev/null)
    if [ "$got" = "fake-hc-tui 0.1.0 $3" ]; then
        ok "$1/$2 installs $3"
    else
        bad "$1/$2 installs $3 — got '$got'"
    fi
    [ -x "$work/$dest/hc" ] && ok "$1/$2 installs the hc core" \
        || bad "$1/$2 core install — out: $out"
}

check_platform Linux x86_64 x86_64-unknown-linux-musl
check_platform Linux aarch64 aarch64-unknown-linux-musl
check_platform Darwin arm64 aarch64-apple-darwin
check_platform Darwin x86_64 x86_64-apple-darwin

# --- checksum mismatch installs nothing --------------------------------------
bad_sums=$work/bad-release
mkdir -p "$bad_sums/download/v9.9.9"
cp "$FIX/download/v9.9.9/hc-tui-9.9.9-x86_64-unknown-linux-musl.tar.gz" \
    "$bad_sums/download/v9.9.9/"
echo "0000000000000000000000000000000000000000000000000000000000000000  hc-tui-9.9.9-x86_64-unknown-linux-musl.tar.gz" \
    >"$bad_sums/download/v9.9.9/sha256sums.txt"
out=$(env -i PATH="$BIN:$SYSBIN" HOME="$work/home" \
    FAKE_SYS=Linux FAKE_MACH=x86_64 \
    HC_TUI_BASE_URL="file://$bad_sums" HC_TUI_INSTALL_DIR="$work/mismatch" \
    HC_TUI_VERSION=v9.9.9 sh "$INSTALLER" 2>&1)
st=$?
[ $st -ne 0 ] && want "mismatch reports checksum" "$out" "checksum mismatch"
[ ! -e "$work/mismatch/hc-tui" ] && ok "mismatch installs nothing" \
    || bad "mismatch left a binary in $work/mismatch"
[ -z "$(ls "$work/mismatch"/.hc-tui.tmp.* 2>/dev/null)" ] \
    && ok "mismatch leaves no tmp file" || bad "mismatch left tmp file"

# --- unsupported platform fails cleanly --------------------------------------
out=$(run unsupported FreeBSD amd64 HC_TUI_VERSION=v0.1.0)
st=$?
if [ $st -ne 0 ]; then
    want "unsupported names the platform" "$out" "FreeBSD/amd64"
else
    bad "unsupported platform installed: $out"
fi
[ ! -e "$work/unsupported/hc-tui" ] || bad "unsupported left a binary"

out=$(run unsp2 Linux mips HC_TUI_VERSION=v0.1.0)
st=$?
if [ $st -ne 0 ]; then
    want "unsupported arch fails" "$out" "no prebuilt"
else
    bad "mips installed: $out"
fi

# --- latest release resolution ------------------------------------------------
# The stub mirrors real curl: without -L a /latest request answers the
# URL it was given, so the installer cannot see the tag.
nol=$(HC_TUI_FAKE_REDIRECT="file://$FIX/tag/v9.9.9" \
    "$BIN/curl" -fsSI -o /dev/null -w '%{url_effective}' "file://$FIX/latest")
wit=$(HC_TUI_FAKE_REDIRECT="file://$FIX/tag/v9.9.9" \
    "$BIN/curl" -fsSIL -o /dev/null -w '%{url_effective}' "file://$FIX/latest")
[ "$nol" = "file://$FIX/latest" ] && [ "$wit" = "file://$FIX/tag/v9.9.9" ] \
    && ok "stub curl only follows with -L" \
    || bad "stub curl: no-L '$nol', with-L '$wit'"

out=$(run latest Linux x86_64 HC_TUI_FAKE_REDIRECT="file://$FIX/tag/v9.9.9")
st=$?
got=$(cat "$work/latest/hc-tui" 2>/dev/null)
[ $st -eq 0 ] && [ "$got" = "fake-hc-tui 9.9.9 x86_64-unknown-linux-musl" ] \
    && ok "latest resolves and installs" \
    || bad "latest — exit $st, binary '$got', out: $out"

# --- version without leading v ------------------------------------------------
out=$(run nov Linux x86_64 HC_TUI_VERSION=0.1.0)
st=$?
got=$(cat "$work/nov/hc-tui" 2>/dev/null)
[ $st -eq 0 ] && [ "$got" = "fake-hc-tui 0.1.0 x86_64-unknown-linux-musl" ] \
    && ok "bare version installs" || bad "bare version — exit $st: $out"

# --- missing archive entry in sha256sums -------------------------------------
empty_sums=$work/empty-release
mkdir -p "$empty_sums/download/v0.1.0"
cp "$FIX/download/v0.1.0/hc-tui-0.1.0-x86_64-unknown-linux-musl.tar.gz" \
    "$empty_sums/download/v0.1.0/"
echo "1111111111111111111111111111111111111111111111111111111111111111  other.tar.gz" \
    >"$empty_sums/download/v0.1.0/sha256sums.txt"
out=$(env -i PATH="$BIN:$SYSBIN" HOME="$work/home" \
    FAKE_SYS=Linux FAKE_MACH=x86_64 \
    HC_TUI_BASE_URL="file://$empty_sums" HC_TUI_INSTALL_DIR="$work/nosum" \
    HC_TUI_VERSION=v0.1.0 sh "$INSTALLER" 2>&1)
st=$?
if [ $st -ne 0 ]; then
    want "unlisted archive fails" "$out" "not listed"
else
    bad "unlisted archive installed: $out"
fi
[ ! -e "$work/nosum/hc-tui" ] || bad "unlisted archive left a binary"

# --- PATH warning and default install dir -------------------------------------
out=$(run pathwarn Linux x86_64 HC_TUI_VERSION=v0.1.0 PATH="$BIN:$SYSBIN")
want "off-PATH install warns" "$out" "not on your PATH"

out=$(env -i PATH="$BIN:$SYSBIN" HOME="$work/defaulthome" \
    FAKE_SYS=Linux FAKE_MACH=x86_64 \
    HC_TUI_BASE_URL="file://$FIX" HC_TUI_SKIP_CORE=1 \
    HC_TUI_VERSION=v0.1.0 sh "$INSTALLER" 2>&1)
st=$?
[ $st -eq 0 ] && [ -f "$work/defaulthome/.local/bin/hc-tui" ] \
    && ok "default install dir is ~/.local/bin" \
    || bad "default dir — exit $st: $out"

# --- sha256 tool fallback (no sha256sum/shasum on PATH) ------------------------
MINBIN=$work/minbin
mkdir -p "$MINBIN"
for tool in sh tar gzip awk mktemp rm cp chmod mv mkdir openssl cat; do
    src=$(command -v "$tool" 2>/dev/null) && ln -sf "$src" "$MINBIN/$tool"
done
cp "$BIN/uname" "$BIN/curl" "$MINBIN/"
chmod 755 "$MINBIN/uname" "$MINBIN/curl"
out=$(run minpath Linux x86_64 HC_TUI_VERSION=v0.1.0 PATH="$MINBIN")
st=$?
got=$(cat "$work/minpath/hc-tui" 2>/dev/null)
[ $st -eq 0 ] && [ "$got" = "fake-hc-tui 0.1.0 x86_64-unknown-linux-musl" ] \
    && ok "installs with openssl-only sha256" \
    || bad "openssl fallback — exit $st: $out"

# --- hc core install ----------------------------------------------------------
# Missing everywhere -> the fixture core installer runs and drops an hc in
# the install dir.
out=$(run core-missing Linux x86_64 HC_TUI_VERSION=v0.1.0)
st=$?
[ $st -eq 0 ] && [ -x "$work/core-missing/hc" ] \
    && want "missing hc installs the core" "$out" "installing the hc core" \
    || bad "missing hc — exit $st: $out"

# Present in any of the three places -> no core install.
extrabin=$work/extra-bin
mkdir -p "$extrabin"
printf '#!/bin/sh\n' >"$extrabin/hc"
chmod 755 "$extrabin/hc"
out=$(run core-onpath Linux x86_64 HC_TUI_VERSION=v0.1.0 \
    PATH="$extrabin:$BIN:$SYSBIN")
st=$?
{ [ $st -eq 0 ] && [ ! -e "$work/core-onpath/hc" ] \
    && case "$out" in *"installing the hc core"*) false ;; *) true ;; esac; } \
    && ok "hc on PATH skips the core installer" \
    || bad "hc on PATH — exit $st: $out"

mkdir -p "$work/core-indir"
printf '#!/bin/sh\n' >"$work/core-indir/hc"
chmod 755 "$work/core-indir/hc"
out=$(run core-indir Linux x86_64 HC_TUI_VERSION=v0.1.0)
st=$?
{ [ $st -eq 0 ] && case "$out" in *"installing the hc core"*) false ;; *) true ;; esac; } \
    && ok "hc in install dir skips the core installer" \
    || bad "hc in install dir — exit $st: $out"

mkdir -p "$work/home-hc/.local/bin"
printf '#!/bin/sh\n' >"$work/home-hc/.local/bin/hc"
chmod 755 "$work/home-hc/.local/bin/hc"
out=$(run core-localbin Linux x86_64 HC_TUI_VERSION=v0.1.0 HOME="$work/home-hc")
st=$?
{ [ $st -eq 0 ] && [ ! -e "$work/core-localbin/hc" ] \
    && case "$out" in *"installing the hc core"*) false ;; *) true ;; esac; } \
    && ok "hc in ~/.local/bin skips the core installer" \
    || bad "hc in ~/.local/bin — exit $st: $out"

# Opt-out skips it too.
out=$(run core-skip Linux x86_64 HC_TUI_VERSION=v0.1.0 HC_TUI_SKIP_CORE=1)
st=$?
{ [ $st -eq 0 ] && [ ! -e "$work/core-skip/hc" ] \
    && case "$out" in *"installing the hc core"*) false ;; *) true ;; esac; } \
    && ok "HC_TUI_SKIP_CORE skips the core installer" \
    || bad "HC_TUI_SKIP_CORE — exit $st: $out"

# A failing core installer warns but keeps hc-tui and exits 0.
out=$(run core-fail Linux x86_64 HC_TUI_VERSION=v0.1.0 \
    HC_TUI_CORE_URL="file://$FIX/core-fail.sh")
st=$?
[ $st -eq 0 ] || bad "core failure exits nonzero — exit $st: $out"
[ -f "$work/core-fail/hc-tui" ] \
    && ok "failed core install keeps hc-tui" \
    || bad "failed core install lost hc-tui"
want "failed core warns" "$out" "core installer failed"
want "failed core prints the manual command" "$out" \
    "curl -fsSL https://releases.herdr.dev/core/install.sh | sh"

echo "---"
echo "$pass passed, $fail failed"
[ "$fail" -eq 0 ]
