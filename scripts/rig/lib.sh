# Shared validation and process helpers for rig.sh (sourced).
# Globals such as ROOT, MACHINES and SEED_LATEST belong to rig.sh.

die() {
    printf 'rig: %s\n' "$*" >&2
    exit 2
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

check_size() {
    [[ $SIZE =~ ^[0-9]+x[0-9]+$ ]] || die "bad --size '$SIZE', want WxH"
}

check_display() {
    local d=$1
    [[ $d =~ ^:[0-9]+(\.[0-9]+)?$ ]] || die "bad --display '$d', want :N"
}

check_machines() {
    case "$MACHINES" in
        1 | 2 | 3) ;;
        *) die "bad --machines '$MACHINES', want 1, 2 or 3" ;;
    esac
}

check_seed() {
    [[ $SEED_LATEST =~ ^v?[0-9]+\.[0-9]+\.[0-9]+$ ]] ||
        die "bad --seed-latest '$SEED_LATEST', want vX.Y.Z"
}

have() { command -v "$1" >/dev/null 2>&1; }

# Env arrays are built per machine: env -i guarantees no inherited
# HC_*, HERDR_* or BACKLOT_* leaks into a rig process. The fake runner
# is the one BACKLOT_* value passed through explicitly (rig.sh resolves
# it), so an installed backlot finds its checkout's script.
ENVV=()
core_env() {
    local l=$1
    ENVV=(env -i
        "PATH=$ROOT/bin-$l:/usr/bin:/bin"
        "HOME=$ROOT/home-$l"
        "XDG_STATE_HOME=$ROOT/state-$l"
        "XDG_CONFIG_HOME=$ROOT/config-$l"
        "XDG_RUNTIME_DIR=$ROOT/run-$l"
        "TMPDIR=$ROOT/tmp"
        "HC_SESSION=rig-$l"
        "HC_SOCKET=$ROOT/core-$l.sock"
        "HERDR_DISABLE_SOUND=1"
        "BACKLOT_NO_UPDATE_CHECK=1"
        "BACKLOT_STATE_DIR=$ROOT/backlot-$l"
        "BACKLOT_FAKE_RUNNER=$FAKE_RUNNER"
        "LANG=C.UTF-8"
        "TERM=xterm-256color"
        "SHELL=/bin/sh")
}

record_pid() {
    printf '%s %s\n' "$1" "$2" >>"$ROOT/pids"
}

wait_socket() {
    local path=$1 i=0
    while [ "$i" -lt 200 ]; do
        if [ -S "$path" ]; then return 0; fi
        sleep 0.1
        i=$((i + 1))
    done
    die "socket never appeared: $path"
}

# A zombie is already dead: the parent may not have reaped it yet.
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
