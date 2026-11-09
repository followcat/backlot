#!/bin/sh
# Fetches the Zig packages that crates/hc-vt/build.rs needs into
# GHOSTTY_DIR/zig-pkg, retrying past flaky mirrors (codeberg 504s,
# dropped connections to deps.files.ghostty.org). CI runs it before
# cargo triggers the real zig build; see .github/actions/zig-deps.
#
#   tools/fetch-zig-deps.sh ZIG [GHOSTTY_DIR]
#
# GHOSTTY_DIR defaults to the vendored submodule. ZIG_TARGET may name a
# Zig -Dtarget triple (e.g. x86_64-linux-musl) matching the cross build
# build.rs will run. FETCH_ATTEMPTS, FETCH_TIMEOUT and FETCH_BACKOFF
# (seconds) tune the retry loop; tests set them small.
set -u

ROOT=$(CDPATH= cd -- "$(dirname -- "$0")/.." && pwd)
ZIG=${1:?usage: fetch-zig-deps.sh ZIG [GHOSTTY_DIR]}
DIR=${2:-$ROOT/vendor/ghostty}

ATTEMPTS=${FETCH_ATTEMPTS:-5}
TIMEOUT=${FETCH_TIMEOUT:-600}
BACKOFF=${FETCH_BACKOFF:-15}

cd "$DIR" || exit 1

TIMEDOUT=$(mktemp "${TMPDIR:-/tmp}/fetch-zig-deps.XXXXXX")
rm -f "$TIMEDOUT"
trap 'rm -f "$TIMEDOUT"' EXIT
trap 'exit 1' INT TERM

# A real `zig build` with the flags crates/hc-vt/build.rs uses: lazy
# dependencies are only requested while build.zig runs, which --fetch
# alone never does. Prefix and local cache go to temp so the vendored
# tree only gains zig-pkg.
set -- build -Demit-lib-vt -Doptimize=ReleaseFast -Dcpu=baseline \
    --prefix "${TMPDIR:-/tmp}/hc-tui-zig-fetch-out" \
    --cache-dir "${TMPDIR:-/tmp}/hc-tui-zig-fetch"
if [ "${ZIG_TARGET:-}" ]; then
    set -- "$@" "-Dtarget=$ZIG_TARGET"
fi

# Zig gets its own process group so a stuck fetch can be stopped whole:
# setsid where it exists (Linux), a python setpgid shim elsewhere
# (macOS runners ship python3), a lone child as last resort.
spawn() {
    if command -v setsid >/dev/null 2>&1; then
        setsid "$ZIG" "$@" &
    elif command -v python3 >/dev/null 2>&1; then
        python3 -c 'import os, sys
os.setpgid(0, 0)
os.execvp(sys.argv[1], sys.argv[1:])' "$ZIG" "$@" &
    else
        "$ZIG" "$@" &
    fi
}

# TERM one attempt's process group, then KILL it shortly after; the
# group form kills any build-runner child zig spawned too.
stop() {
    kill -s TERM -- -"$1" 2>/dev/null || kill -s TERM "$1" 2>/dev/null
}
stop_hard() {
    kill -s KILL -- -"$1" 2>/dev/null || kill -s KILL "$1" 2>/dev/null
}

# One bounded attempt: zig's exit status, or 124 when TIMEOUT forced the
# stop. GNU timeout(1) is absent on macOS, so the watchdog is a shell
# job; its inner sleep dies with it through the EXIT trap.
attempt() {
    spawn "$@"
    pid=$!
    (
        trap 'kill "$nap" 2>/dev/null' EXIT
        sleep "$TIMEOUT" & nap=$!
        wait "$nap" 2>/dev/null
        stop "$pid" || exit 0
        : >"$TIMEDOUT"
        sleep 10 & nap=$!
        wait "$nap" 2>/dev/null
        stop_hard "$pid"
    ) &
    dog=$!
    wait "$pid" 2>/dev/null
    rc=$?
    kill "$dog" 2>/dev/null
    wait "$dog" 2>/dev/null
    if [ -f "$TIMEDOUT" ]; then
        rm -f "$TIMEDOUT"
        [ "$rc" -ne 0 ] && return 124
    fi
    return "$rc"
}

n=1
while [ "$n" -le "$ATTEMPTS" ]; do
    echo "fetch-zig-deps: attempt $n of $ATTEMPTS"
    attempt "$@"
    rc=$?
    if [ "$rc" -eq 0 ]; then
        [ -d zig-pkg ] || {
            echo "fetch-zig-deps: fetch succeeded but $DIR/zig-pkg is missing" >&2
            exit 1
        }
        count=$(ls zig-pkg | wc -l | tr -d ' ')
        size=$(du -sh zig-pkg | cut -f1)
        echo "fetch-zig-deps: $count packages, $size in $DIR/zig-pkg"
        exit 0
    fi
    if [ "$n" -lt "$ATTEMPTS" ]; then
        delay=$((BACKOFF * n))
        [ "$rc" -eq 124 ] \
            && echo "fetch-zig-deps: attempt $n hit the ${TIMEOUT}s timeout" >&2 \
            || echo "fetch-zig-deps: attempt $n failed (status $rc)" >&2
        echo "fetch-zig-deps: retrying in ${delay}s" >&2
        sleep "$delay"
    fi
    n=$((n + 1))
done

echo "fetch-zig-deps: failed after $ATTEMPTS attempts" >&2
exit 1
