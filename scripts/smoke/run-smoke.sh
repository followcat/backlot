#!/usr/bin/env bash
# Opt-in single-task studio smoke run.
#
# Creates a temporary neutral repository and a bounded fake or real cast,
# runs `studio new --dry-run`, then drives one task through a scratch core,
# and always cleans up. No network is used by `--runner fake`.
#
#   scripts/smoke/run-smoke.sh --dry-run
#   scripts/smoke/run-smoke.sh --runner fake
#   scripts/smoke/run-smoke.sh --runner claude --model claude-haiku-5-5 --timeout 900
#
# Options: --runner fake|claude|codex|devin|cursor|cmd|opencode
#          --model M --effort low|medium|high|xhigh|max
#          --timeout SECS (default 900) --root DIR --artifacts DIR
#          --backlot BIN --core BIN --hc BIN
#          --dry-run --print-cast --keep-root
set -euo pipefail

SELF_DIR=$(CDPATH= cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)
# shellcheck source=scripts/smoke/fixture.sh
source "$SELF_DIR/fixture.sh"

RUNNER=fake
MODEL=
EFFORT=
TIMEOUT=900
ROOT=
ARTIFACTS=
BACKLOT_BIN=${BACKLOT_BIN:-}
CORE_BIN=${HC_CORE_BIN:-}
HC_BIN=${HC_BIN:-}
DRY_RUN=0
PRINT_CAST=0
KEEP_ROOT=0
STUDIO=acme-shop

say() { printf 'smoke: %s\n' "$*" >&2; }
die() { printf 'smoke: %s\n' "$*" >&2; exit 2; }

usage() {
    cat <<'EOF'
run-smoke.sh [options]

  --runner R   fake (default), claude, codex, devin, cursor, cmd, opencode
  --model M    model name; cheap defaults for fake/claude/codex/cursor
  --effort E   low, medium, high, xhigh or max (not for cursor/devin/cmd)
  --timeout S  wall-clock bound in seconds (default 900)
  --root DIR   scratch root (default: mktemp under TMPDIR)
  --artifacts DIR  copy the ledger, status, cast and dry-run log here
  --backlot BIN --core BIN --hc BIN
  --dry-run    generate the cast and run `studio new --dry-run` only
  --print-cast print the generated cast on stdout (needs --dry-run)
  --keep-root  keep the scratch root for inspection
EOF
}

safe_root() {
    local r=${1%/}
    [ -n "$r" ] || die 'refusing an empty --root'
    case "$r" in
        /) die "refusing unsafe --root: $1" ;;
        "$HOME") die "refusing unsafe --root: $1 (the real HOME)" ;;
    esac
    ROOT=$r
}

while [ $# -gt 0 ]; do
    case "$1" in
        --runner) RUNNER=${2:?}; shift 2 ;;
        --model) MODEL=${2:-}; shift 2 ;;
        --effort) EFFORT=${2:-}; shift 2 ;;
        --timeout) TIMEOUT=${2:?}; shift 2 ;;
        --root) ROOT=${2:?}; shift 2 ;;
        --artifacts) ARTIFACTS=${2:?}; shift 2 ;;
        --backlot) BACKLOT_BIN=${2:?}; shift 2 ;;
        --core) CORE_BIN=${2:?}; shift 2 ;;
        --hc) HC_BIN=${2:?}; shift 2 ;;
        --dry-run) DRY_RUN=1; shift ;;
        --print-cast) PRINT_CAST=1; shift ;;
        --keep-root) KEEP_ROOT=1; shift ;;
        -h | --help) usage; exit 0 ;;
        *) die "unknown argument $1" ;;
    esac
done

case "$RUNNER" in
    fake | claude | codex | devin | cursor | cmd | opencode) ;;
    *) die "bad --runner '$RUNNER'" ;;
esac
[[ $TIMEOUT =~ ^[0-9]+$ ]] || die "bad --timeout '$TIMEOUT'"
if [ "$TIMEOUT" -lt 30 ] || [ "$TIMEOUT" -gt 7200 ]; then
    die "--timeout must be between 30 and 7200 seconds"
fi
if [ -n "$EFFORT" ]; then
    case "$EFFORT" in
        none | low | medium | high | xhigh | max) ;;
        *) die "bad --effort '$EFFORT'" ;;
    esac
fi
case "$RUNNER" in
    cursor | devin | cmd)
        [ -z "$EFFORT" ] || die "$RUNNER carries its effort in the model id; drop --effort"
        EFFORT=
        ;;
    *)
        EFFORT=${EFFORT:-low}
        ;;
esac
if [ -z "$MODEL" ]; then
    case "$RUNNER" in
        fake) MODEL=model-a ;;
        claude) MODEL=claude-haiku-5-5 ;;
        codex) MODEL=gpt-5.3-codex ;;
        cursor) MODEL=composer-2.5 ;;
        *) die "--runner $RUNNER needs an explicit --model" ;;
    esac
fi
[ -z "$ROOT" ] || safe_root "$ROOT"
if [ -n "$ARTIFACTS" ]; then
    case "${ARTIFACTS%/}" in
        /) die "refusing unsafe --artifacts: $ARTIFACTS" ;;
    esac
fi
[ "$PRINT_CAST" = 0 ] || [ "$DRY_RUN" = 1 ] || die '--print-cast needs --dry-run'

BL=${BACKLOT_BIN:-$(command -v backlot 2>/dev/null || true)}
CORE=${CORE_BIN:-$(command -v herdr-core 2>/dev/null || true)}
[ -n "$CORE" ] || CORE=${HC_BIN:-$(command -v hc 2>/dev/null || true)}
HC=${HC_BIN:-$(command -v hc 2>/dev/null || true)}

if [ "$DRY_RUN" = 0 ]; then
    [ -n "$BL" ] || die 'backlot binary not found; pass --backlot or set BACKLOT_BIN'
    [ -n "$CORE" ] || die 'core binary not found; pass --core or set HC_CORE_BIN'
fi

if [ -z "$ROOT" ]; then
    ROOT=$(mktemp -d "${TMPDIR:-/tmp}/backlot-smoke.XXXXXX")
else
    mkdir -p "$ROOT"
fi
ROOT=$(CDPATH= cd -- "$ROOT" && pwd)
SMOKE_ROOT=$ROOT
SESSION="smoke-$$-$RANDOM"
REPO="$ROOT/repos/$STUDIO"
INPUTS="$ROOT/inputs"
LEDGER="$ROOT/backlot/studios/$STUDIO/events.jsonl"

mkdir -p "$ROOT"/{bin,home,state,config,run,tmp,inputs,repos}
chmod 700 "$ROOT/run" "$ROOT/tmp"
if [ -n "$HC" ]; then ln -sf "$HC" "$ROOT/bin/hc"; fi
if [ -n "$BL" ]; then ln -sf "$BL" "$ROOT/bin/backlot"; fi
mkdir -p "$ROOT/state/herdr-core/$SESSION"
printf '{"id":"%s","name":"host-a"}\n' "$SESSION" \
    >"$ROOT/state/herdr-core/$SESSION/core.json"

ENVV=(env -i
    "PATH=$ROOT/bin:/usr/bin:/bin"
    "HOME=$ROOT/home"
    "XDG_STATE_HOME=$ROOT/state"
    "XDG_CONFIG_HOME=$ROOT/config"
    "XDG_RUNTIME_DIR=$ROOT/run"
    "TMPDIR=$ROOT/tmp"
    "HC_SESSION=$SESSION"
    "HC_SOCKET=$ROOT/core.sock"
    "BACKLOT_NO_UPDATE_CHECK=1"
    "BACKLOT_STATE_DIR=$ROOT/backlot"
    "HERDR_DISABLE_SOUND=1"
    "LANG=C.UTF-8"
    "TERM=xterm-256color"
    "SHELL=/bin/sh")

bl() { "${ENVV[@]}" "$BL" "$@" --studio "$STUDIO"; }
git_in() {
    local dir=$1
    shift
    git -C "$dir" -c user.name=alice -c user.email=alice@example.com "$@"
}

wait_socket() {
    local i=0
    while [ "$i" -lt 200 ]; do
        if [ -S "$ROOT/core.sock" ]; then return 0; fi
        sleep 0.1
        i=$((i + 1))
    done
    die "core socket never appeared"
}

wait_path() {
    local path=$1 i=0
    while [ "$i" -lt 600 ]; do
        if [ -e "$path" ]; then return 0; fi
        sleep 0.2
        i=$((i + 1))
    done
    die "path never appeared: $path"
}

wait_kind() { # kind count
    local kind=$1 want=$2 i=0 got=0
    while [ "$i" -lt 600 ]; do
        got=$(grep -c "\"kind\":\"$kind\"" "$LEDGER" 2>/dev/null || true)
        got=${got:-0}
        if [ "$got" -ge "$want" ]; then return 0; fi
        sleep 0.2
        i=$((i + 1))
    done
    die "ledger never reached $want $kind events"
}

# A zombie is already dead: its parent may not have reaped it yet.
pid_alive() {
    local pid=$1 stat
    if ! kill -0 "$pid" 2>/dev/null; then return 1; fi
    stat=$(ps -o stat= -p "$pid" 2>/dev/null | tr -d ' ')
    if [ -z "$stat" ]; then return 1; fi
    case "$stat" in
        Z*) return 1 ;;
    esac
    return 0
}

run_dry() {
    make_repo
    write_cast
    write_inputs
    if [ -n "$BL" ]; then
        if ! "${ENVV[@]}" "$BL" studio new --studio "$STUDIO" --title "$STUDIO" \
            --repo "$REPO" --cast "$ROOT/cast.toml" --dry-run \
            >"$ROOT/smoke-dry-run.txt" 2>&1; then
            cat "$ROOT/smoke-dry-run.txt" >&2
            die 'studio new --dry-run refused the generated cast'
        fi
        cat "$ROOT/smoke-dry-run.txt" >&2
    else
        say 'backlot binary not found; skipped studio new --dry-run'
    fi
    say "runner=$RUNNER model=$MODEL effort=${EFFORT:-none} timeout=$TIMEOUT root=$ROOT"
    if [ "$PRINT_CAST" = 1 ]; then
        cat "$ROOT/cast.toml"
    else
        printf 'smoke dry-run: runner=%s model=%s timeout=%s\n' "$RUNNER" "$MODEL" "$TIMEOUT"
    fi
}

run_steps() {
    "${ENVV[@]}" "$CORE" serve >"$ROOT/log-core.txt" 2>&1 &
    printf 'core %s\n' "$!" >"$ROOT/pids"
    wait_socket

    bl studio new --title "$STUDIO" --repo "$REPO" --cast "$ROOT/cast.toml" >/dev/null
    bl stage start >/dev/null
    wait_kind role.started 2

    bl req revise --file "$INPUTS/req.md" --summary v1 --as producer >/dev/null
    bl plan request --as producer >/dev/null
    wait_kind role.started 3
    bl plan publish --kind plan --file "$INPUTS/plan.md" --as writer >/dev/null
    bl plan publish --kind acceptance --file "$INPUTS/acc.md" --as writer >/dev/null
    bl dispatch --file "$INPUTS/note.md" --as producer >/dev/null
    bl task new --title shop-total --kind feature \
        --acceptance-file "$INPUTS/acc.md" --as director >/dev/null
    bl cast --task t1 --actor actor-a --reason smoke --as director >/dev/null

    local work="$ROOT/backlot/studios/$STUDIO/work/t1"
    wait_path "$work"
    git_in "$work" commit -q --allow-empty -m a
    local a b
    a=$(git_in "$work" rev-parse HEAD)
    git_in "$work" commit -q --allow-empty -m b
    b=$(git_in "$work" rev-parse HEAD)
    bl deliver --commits "$a..$b" --report "$INPUTS/report.md" \
        --as actor-t1-c1 --task t1 >/dev/null
    bl review --task t1 --delivery 1 --verdict accepted \
        --conclusion smoke --as director >/dev/null
    bl studio finish --reason smoke --as producer >/dev/null
    wait_kind studio.finished 1
    say 'studio finished one accepted task'
}

copy_artifacts() {
    [ -n "$ARTIFACTS" ] || return 0
    mkdir -p "$ARTIFACTS"
    cp "$ROOT/cast.toml" "$ARTIFACTS/cast.toml" 2>/dev/null || true
    [ -f "$ROOT/smoke-dry-run.txt" ] &&
        cp "$ROOT/smoke-dry-run.txt" "$ARTIFACTS/dry-run.txt" || true
    [ -f "$LEDGER" ] && cp "$LEDGER" "$ARTIFACTS/events.jsonl" || true
    if [ -n "$BL" ] && [ -S "$ROOT/core.sock" ]; then
        "${ENVV[@]}" "$BL" status --all >"$ARTIFACTS/status.txt" 2>&1 || true
    fi
    {
        printf 'runner=%s\nmodel=%s\neffort=%s\ntimeout=%s\nroot=%s\n' \
            "$RUNNER" "$MODEL" "${EFFORT:-none}" "$TIMEOUT" "$ROOT"
    } >"$ARTIFACTS/smoke.txt"
}

stop_core() {
    if [ -n "$BL" ] && [ -S "$ROOT/core.sock" ]; then
        "${ENVV[@]}" "$BL" stage stop --studio "$STUDIO" >/dev/null 2>&1 || true
    fi
    if [ -n "$CORE" ] && [ -S "$ROOT/core.sock" ]; then
        "${ENVV[@]}" "$CORE" --json server stop >/dev/null 2>&1 || true
    fi
    if [ -f "$ROOT/pids" ]; then
        local name pid
        while read -r name pid; do
            if pid_alive "$pid"; then
                kill -TERM "$pid" 2>/dev/null || true
            fi
        done <"$ROOT/pids"
        sleep 0.5
        while read -r name pid; do
            if pid_alive "$pid"; then
                kill -KILL "$pid" 2>/dev/null || true
            fi
        done <"$ROOT/pids"
    fi
}

on_exit() {
    local rc=$?
    if [ -n "${ROOT:-}" ] && [ -d "$ROOT" ]; then
        copy_artifacts || true
        stop_core || true
        if [ "$KEEP_ROOT" = 1 ]; then
            say "kept root $SMOKE_ROOT"
        elif ! rm -rf "${SMOKE_ROOT:?}"; then
            say "could not remove $SMOKE_ROOT"
            rc=1
        fi
    fi
    exit "$rc"
}
trap on_exit EXIT

if [ "$DRY_RUN" = 1 ]; then
    run_dry
    exit 0
fi

make_repo
write_cast
write_inputs
if ! "${ENVV[@]}" "$BL" studio new --studio "$STUDIO" --title "$STUDIO" \
    --repo "$REPO" --cast "$ROOT/cast.toml" --dry-run >"$ROOT/smoke-dry-run.txt" 2>&1; then
    cat "$ROOT/smoke-dry-run.txt" >&2
    die 'studio new --dry-run refused the generated cast'
fi

( run_steps ) &
child=$!
(
    sleep "$TIMEOUT"
    if kill -0 "$child" 2>/dev/null; then
        : >"$ROOT/timed-out"
        say "timeout after ${TIMEOUT}s"
        kill -TERM "$child" 2>/dev/null || true
        sleep 3
        kill -KILL "$child" 2>/dev/null || true
    fi
) &
watchdog=$!
rc=0
wait "$child" || rc=$?
kill "$watchdog" 2>/dev/null || true
wait "$watchdog" 2>/dev/null || true
if [ -f "$ROOT/timed-out" ]; then
    rc=124
    say 'timed out'
elif [ "$rc" -eq 0 ]; then
    say 'smoke passed'
else
    say "smoke failed (exit $rc)"
fi
exit "$rc"
