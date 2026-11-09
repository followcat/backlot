#!/bin/sh
# Offline tests for tools/fetch-zig-deps.sh. A stub zig binary plays the
# fetch: flaky fails a fixed number of times before succeeding, dead
# always fails, hang sleeps past the attempt timeout. The stub records
# its arguments and call count; no network is touched.
# Run: sh tools/test-fetch-zig-deps.sh
set -u

ROOT=$(CDPATH= cd -- "$(dirname -- "$0")/.." && pwd)
FETCHER=$ROOT/tools/fetch-zig-deps.sh
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

# --- fixture ---------------------------------------------------------------
# A scratch "ghostty" tree: fetch-zig-deps.sh only needs a directory it
# can cd into; the stub zig fills zig-pkg when it "succeeds".
GDIR=$work/ghostty
mkdir -p "$GDIR"

BIN=$work/bin
mkdir -p "$BIN"
cat >"$BIN/zig" <<'EOF'
#!/bin/sh
n=$(cat "$FAKE_COUNT" 2>/dev/null || echo 0)
n=$((n + 1))
echo "$n" >"$FAKE_COUNT"
printf '%s\n' "$@" >"$FAKE_ARGS"
case "${FAKE_MODE:-ok}" in
    ok)
        mkdir -p zig-pkg/pkg-a
        ;;
    flaky)
        if [ "$n" -le "${FAKE_FAILS:-2}" ]; then
            echo "stub zig: fetch failed (attempt $n)" >&2
            exit 1
        fi
        mkdir -p zig-pkg/pkg-a
        ;;
    dead)
        echo "stub zig: fetch failed" >&2
        exit 1
        ;;
    hang)
        sleep 3600
        ;;
esac
EOF
chmod 755 "$BIN/zig"

# run MODE [VAR=val ...] - the fetcher under the stub env: tiny retry
# knobs keep the test fast.
run() {
    mode=$1
    shift
    rm -f "$work/count" "$work/args"
    rm -rf "$GDIR/zig-pkg"
    env -i PATH="$BIN:$SYSBIN" HOME="$work/home" \
        FAKE_MODE=$mode FAKE_COUNT="$work/count" FAKE_ARGS="$work/args" \
        FETCH_ATTEMPTS=4 FETCH_TIMEOUT=3 FETCH_BACKOFF=1 \
        "$@" sh "$FETCHER" "$BIN/zig" "$GDIR" 2>&1
}

# --- immediate success ------------------------------------------------------
out=$(run ok)
st=$?
[ $st -eq 0 ] && [ -d "$GDIR/zig-pkg/pkg-a" ] \
    && ok "clean fetch populates zig-pkg" \
    || bad "clean fetch - exit $st: $out"
[ "$(cat "$work/count")" = 1 ] \
    && ok "clean fetch takes one attempt" \
    || bad "clean fetch attempt count: $(cat "$work/count")"
case "$out" in
    *"1 packages"*) ok "success reports the package count" ;;
    *) bad "missing package count in: $out" ;;
esac

# --- retries through failures ------------------------------------------------
out=$(run flaky FAKE_FAILS=2)
st=$?
[ $st -eq 0 ] && [ "$(cat "$work/count")" = 3 ] \
    && ok "flaky fetch succeeds on attempt 3" \
    || bad "flaky fetch - exit $st, count $(cat "$work/count" 2>/dev/null): $out"
case "$out" in
    *"attempt 1 of 4"*"attempt 3 of 4"*) ok "retries are announced" ;;
    *) bad "missing attempt log: $out" ;;
esac

# --- permanent failure reports clearly ---------------------------------------
out=$(run dead)
st=$?
[ $st -ne 0 ] && [ "$(cat "$work/count")" = 4 ] \
    && ok "dead fetch gives up after all attempts" \
    || bad "dead fetch - exit $st, count $(cat "$work/count" 2>/dev/null): $out"
case "$out" in
    *"failed after 4 attempts"*) ok "final failure says so" ;;
    *) bad "missing final failure message: $out" ;;
esac

# --- a hung fetch is timed out and retried ------------------------------------
start=$(date +%s)
out=$(run hang)
st=$?
elapsed=$(( $(date +%s) - start ))
[ $st -ne 0 ] && [ "$(cat "$work/count")" = 4 ] \
    && ok "hung fetch fails after all attempts" \
    || bad "hung fetch - exit $st, count $(cat "$work/count" 2>/dev/null): $out"
# Each attempt ends near FETCH_TIMEOUT=3s, never near the stub's hour.
[ "$elapsed" -lt 60 ] \
    && ok "hung fetch is bounded (${elapsed}s)" \
    || bad "hung fetch took ${elapsed}s"
case "$out" in
    *timeout*) ok "timeout is reported" ;;
    *) bad "missing timeout report: $out" ;;
esac

# --- the zig command line mirrors crates/hc-vt/build.rs ------------------------
# A real build, not --fetch: lazy dependencies are only requested while
# build.zig runs. The stub records one argument per line.
run ok >/dev/null
want_args="build -Demit-lib-vt -Doptimize=ReleaseFast -Dcpu=baseline --prefix --cache-dir"
miss=
for flag in $want_args; do
    grep -qx -- "$flag" "$work/args" || miss="$miss $flag"
done
[ -z "$miss" ] \
    && ok "fetch runs a real build with build.rs flags" \
    || bad "missing flags:$miss in: $(cat "$work/args")"
grep -q '^--fetch' "$work/args" \
    && bad "fetch should not use --fetch: $(cat "$work/args")" \
    || ok "fetch does not use --fetch"
grep -q '^-Dtarget' "$work/args" \
    && bad "native fetch got an unexpected -Dtarget: $(cat "$work/args")" \
    || ok "native fetch passes no -Dtarget"

run ok ZIG_TARGET=x86_64-linux-musl >/dev/null
grep -qx -- "-Dtarget=x86_64-linux-musl" "$work/args" \
    && ok "ZIG_TARGET becomes -Dtarget" \
    || bad "ZIG_TARGET not forwarded: $(cat "$work/args")"

# --- workflow files still parse as YAML ---------------------------------------
if python3 -c 'import yaml' 2>/dev/null; then
    if python3 - "$ROOT" <<'EOF'
import glob, os, sys, yaml
root = sys.argv[1]
files = glob.glob(os.path.join(root, ".github/workflows/*.yml")) + \
    glob.glob(os.path.join(root, ".github/actions/*/action.yml"))
for f in files:
    with open(f) as fh:
        yaml.safe_load(fh)
print("\n".join(os.path.relpath(f, root) for f in files))
EOF
    then
        ok "workflow and action files parse as YAML"
    else
        bad "YAML parse failed"
    fi
else
    echo "skip - python3 without pyyaml; YAML parse check skipped"
fi

echo "---"
echo "$pass passed, $fail failed"
[ "$fail" -eq 0 ]
