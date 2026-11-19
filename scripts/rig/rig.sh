#!/usr/bin/env bash
# Isolated two-machine visual test rig for backlot.
#
# Every process a rig owns runs with cleared HC_*/HERDR_*/BACKLOT_* state
# under a private ROOT: one core per machine, private HOME and XDG trees,
# a private X server and a private kitty socket. Teardown kills only the
# PIDs this rig recorded in $ROOT/pids.
#
#   scripts/rig/rig.sh up [options]       start the rig
#   scripts/rig/rig.sh plan [options]     print what up would do
#   scripts/rig/rig.sh up --dry-run ...   same as plan
#   scripts/rig/rig.sh shot out.png       capture the rig display
#   scripts/rig/rig.sh keys ctrl+b c      send keys through the rig's kitty
#   scripts/rig/rig.sh status             print PIDs, cores, sessions, studios
#   scripts/rig/rig.sh down [--keep-root] stop recorded PIDs and remove ROOT
#
# Options: --root DIR --display :N --size WxH --machines 1|2|3
#          --backlot BIN --hc BIN --core BIN --seed-latest vX.Y.Z
#          --no-gui --keep-root --dry-run
#
# Env equivalents: RIG_ROOT RIG_DISPLAY RIG_SIZE RIG_MACHINES
#                  BACKLOT_BIN HC_BIN HC_CORE_BIN
set -euo pipefail

SELF_DIR=$(CDPATH= cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)
# shellcheck source=scripts/rig/lib.sh
source "$SELF_DIR/lib.sh"
# shellcheck source=scripts/rig/world.sh
source "$SELF_DIR/world.sh"

# Inherited state that must never reach a rig process.
STRIP_VARS="HC_SCOPE HC_TERMINAL_ID HC_TERMINAL_SOCKET HERDR_ENV WAYLAND_DISPLAY BACKLOT_STUDIO BACKLOT_ROLE BACKLOT_TASK BACKLOT_STATE_DIR"

ROOT=${RIG_ROOT:-}
DISPLAY_NAME=${RIG_DISPLAY:-}
SIZE=${RIG_SIZE:-1600x1000}
MACHINES=${RIG_MACHINES:-2}
BACKLOT_BIN=${BACKLOT_BIN:-}
HC_BIN=${HC_BIN:-}
CORE_BIN=${HC_CORE_BIN:-}
SEED_LATEST=
DRY_RUN=0
KEEP_ROOT=0
GUI=1
CROP=
TEXT_MODE=0
CMD=
ARGS=()

usage() {
    cat <<'EOF'
rig.sh up|plan|shot|keys|status|down [options]

  up       start the isolated rig
  plan     print every path and command without spawning anything
  shot     capture the rig display: shot <out.png> [--crop WxH+X+Y]
  keys     send keys to the rig's kitty: keys ctrl+b c, or keys --text ...
  status   print PIDs, sessions, machines and studios
  down     stop only the recorded PIDs and remove ROOT

Options: --root DIR --display :N --size WxH --machines 1|2|3
         --backlot BIN --hc BIN --core BIN --seed-latest vX.Y.Z
         --no-gui --keep-root --dry-run
EOF
}

while [ $# -gt 0 ]; do
    case "$1" in
        up | plan | shot | keys | status | down)
            CMD=$1
            shift
            ;;
        --root) ROOT=${2:?--root needs a value}; shift 2 ;;
        --display) DISPLAY_NAME=${2:?--display needs a value}; shift 2 ;;
        --size) SIZE=${2:?--size needs a value}; shift 2 ;;
        --machines) MACHINES=${2:?--machines needs a value}; shift 2 ;;
        --backlot) BACKLOT_BIN=${2:?}; shift 2 ;;
        --hc) HC_BIN=${2:?}; shift 2 ;;
        --core) CORE_BIN=${2:?}; shift 2 ;;
        --seed-latest) SEED_LATEST=${2:?}; shift 2 ;;
        --crop) CROP=${2:?}; shift 2 ;;
        --text) TEXT_MODE=1; shift ;;
        --keep-root) KEEP_ROOT=1; shift ;;
        --no-gui) GUI=0; shift ;;
        --dry-run) DRY_RUN=1; shift ;;
        -h | --help) usage; exit 0 ;;
        --) shift; ARGS+=("$@"); break ;;
        -*) die "unknown option $1" ;;
        *) ARGS+=("$1"); shift ;;
    esac
done

[ -n "$CMD" ] || { usage >&2; exit 2; }
check_size
check_machines
[ -z "$DISPLAY_NAME" ] || check_display "$DISPLAY_NAME"
[ -z "$SEED_LATEST" ] || check_seed

if [ -n "$ROOT" ]; then safe_root "$ROOT"; fi
if [ -n "$BACKLOT_BIN" ] && [ ! -x "$BACKLOT_BIN" ]; then
    die "--backlot is not executable: $BACKLOT_BIN"
fi
if [ -n "$CORE_BIN" ] && [ ! -x "$CORE_BIN" ]; then
    die "--core is not executable: $CORE_BIN"
fi

BL=${BACKLOT_BIN:-$(command -v backlot 2>/dev/null || true)}
HC=${HC_BIN:-$(command -v hc 2>/dev/null || true)}
CORE=${CORE_BIN:-$(command -v herdr-core 2>/dev/null || true)}
[ -n "$CORE" ] || CORE=$HC
KITTY=$(command -v kitty 2>/dev/null || true)
XVFB=$(command -v Xvfb 2>/dev/null || true)
CAPTURE=$(command -v scrot 2>/dev/null || command -v import 2>/dev/null || true)

RIG_UP=0
on_fail() {
    local rc=$?
    if [ "$RIG_UP" = 1 ] && [ "$rc" -ne 0 ]; then
        printf 'rig: up failed (%s); stopping only recorded PIDs\n' "$rc" >&2
        stop_cores || true
        stop_pids || true
        unmount_runtime || true
        if [ "$KEEP_ROOT" = 0 ] && [ -n "${ROOT:-}" ]; then
            rm -rf "${ROOT:?}" || true
        elif [ -n "${ROOT:-}" ]; then
            printf 'rig: kept %s for inspection\n' "$ROOT" >&2
        fi
    fi
    exit "$rc"
}

do_up() {
    if [ "$DRY_RUN" = 1 ]; then
        print_plan
        return 0
    fi
    [ -n "$BL" ] || die 'backlot binary not found; pass --backlot or set BACKLOT_BIN'
    [ -n "$HC" ] || die 'hc binary not found; pass --hc or set HC_BIN'
    [ -n "$CORE" ] || die 'core binary not found; pass --core or set HC_CORE_BIN'
    if [ "$GUI" = 1 ]; then
        [ -n "$KITTY" ] || die 'kitty not found; install it or pass --no-gui'
        [ -n "$XVFB" ] || die 'Xvfb not found; install it or pass --no-gui'
    fi
    if [ -z "$ROOT" ]; then
        ROOT=$(mktemp -d "${TMPDIR:-/tmp}/backlot-rig.XXXXXX")
    else
        mkdir -p "$ROOT"
        [ -z "$(ls -A "$ROOT")" ] || die "refusing to reuse a non-empty --root: $ROOT"
    fi
    RIG_UP=1
    trap on_fail EXIT

    mkdir -p "$ROOT"/{tmp,repos,via}
    local l
    for l in a b c; do
        mkdir -p "$ROOT/home-$l" "$ROOT/state-$l" "$ROOT/config-$l" "$ROOT/run-$l" "$ROOT/bin-$l"
        chmod 700 "$ROOT/run-$l"
        write_core_json "$l"
        ln -sf "$HC" "$ROOT/bin-$l/hc"
        ln -sf "$CORE" "$ROOT/bin-$l/herdr-core"
        if [ "$l" != c ]; then ln -sf "$BL" "$ROOT/bin-$l/backlot"; fi
        write_via "$l"
    done
    chmod 700 "$ROOT/tmp"

    start_cores
    start_machines
    make_repo acme-shop
    make_repo beta-tools
    write_cast
    start_studios
    seed_latest

    if [ "$GUI" = 1 ]; then
        if [ -z "$DISPLAY_NAME" ]; then pick_display; fi
        start_xvfb
        start_kitty
        printf '%s\n' "$DISPLAY_NAME" >"$ROOT/display"
    fi
    printf 'rig up: root=%s display=%s machines=%s\n' \
        "$ROOT" "${DISPLAY_NAME:-none}" "$MACHINES"
    do_status
}

do_status() {
    [ -n "$ROOT" ] || die 'status needs --root (or RIG_ROOT)'
    printf 'root=%s\n' "$ROOT"
    printf 'display=%s\n' "$(cat "$ROOT/display" 2>/dev/null || printf '%s' "${DISPLAY_NAME:-none}")"
    printf 'size=%s\n' "$SIZE"
    printf 'kitty-socket=%s\n' "$ROOT/kitty.sock"
    local l
    for l in a b c; do
        [ -S "$ROOT/core-$l.sock" ] || continue
        printf 'machine-%s session=rig-%s socket=%s state=%s\n' \
            "$l" "$l" "$ROOT/core-$l.sock" "$ROOT/backlot-$l"
        local sdir="$ROOT/backlot-$l/studios"
        if [ -d "$sdir" ]; then
            printf 'studios-%s=%s\n' "$l" \
                "$(find "$sdir" -mindepth 1 -maxdepth 1 -type d -printf '%f ' 2>/dev/null | sed 's/ $//')"
        fi
    done
    if [ -f "$ROOT/pids" ]; then
        local n p st
        while read -r n p; do
            [ -n "${p:-}" ] || continue
            if pid_alive "$p"; then st=alive; else st=gone; fi
            printf 'pid %-8s %-8s %s\n' "$n" "$p" "$st"
        done <"$ROOT/pids"
    fi
}

do_down() {
    [ -n "$ROOT" ] || die 'down needs --root (or RIG_ROOT)'
    [ -d "$ROOT" ] || die "no such rig root: $ROOT"
    RIG_UP=0
    stop_cores
    if ! stop_pids; then
        die "some recorded processes could not be stopped; kept $ROOT"
    fi
    if [ "$KEEP_ROOT" = 1 ]; then
        printf 'rig down: root kept at %s\n' "$ROOT"
        return 0
    fi
    unmount_runtime
    if ! rm -rf "${ROOT:?}"; then
        die "could not remove $ROOT (a mount may still be held); kept it"
    fi
    printf 'rig down: stopped and removed %s\n' "$ROOT"
}

do_shot() {
    [ -n "$ROOT" ] || die 'shot needs --root (or RIG_ROOT)'
    local out=${ARGS[0]:-}
    [ -n "$out" ] || die 'shot needs an output path: rig.sh shot out.png'
    local disp=${DISPLAY_NAME:-$(cat "$ROOT/display" 2>/dev/null || true)}
    [ -n "$disp" ] || die 'no rig display recorded; was the rig started with a GUI?'
    if have scrot; then
        DISPLAY=$disp scrot -o "$out"
    elif have import; then
        DISPLAY=$disp import -window root "$out"
    else
        die 'no screenshot tool found (scrot or import)'
    fi
    if [ -n "$CROP" ]; then
        python3 - "$out" "$CROP" <<'PY'
import re
import sys
try:
    from PIL import Image
except ImportError:
    sys.exit("rig: --crop needs Pillow (pip install Pillow)")
path, crop = sys.argv[1], sys.argv[2].lower()
m = re.fullmatch(r"(\d+)x(\d+)\+(\d+)\+(\d+)", crop)
if not m:
    sys.exit(f"rig: bad --crop {crop!r}, want WxH+X+Y")
w, h, x, y = (int(v) for v in m.groups())
im = Image.open(path)
im.crop((x, y, x + w, y + h)).save(path)
PY
    fi
    printf 'rig shot: %s\n' "$out"
}

do_keys() {
    [ -n "$ROOT" ] || die 'keys needs --root (or RIG_ROOT)'
    local sock="$ROOT/kitty.sock"
    [ -S "$sock" ] || die "no kitty socket at $sock"
    have kitty || die 'kitty not found on PATH'
    if [ "$TEXT_MODE" = 1 ]; then
        kitty @ --to "unix:$sock" send-text "${ARGS[*]}"
    else
        kitty @ --to "unix:$sock" send-key "${ARGS[@]}"
    fi
}

case "$CMD" in
    up) do_up ;;
    plan) DRY_RUN=1; do_up ;;
    status) do_status ;;
    down) do_down ;;
    shot) do_shot ;;
    keys) do_keys ;;
    *) die "unknown command $CMD" ;;
esac
