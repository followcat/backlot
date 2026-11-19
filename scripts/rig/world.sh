# World construction and teardown for rig.sh (sourced).
# Every function reads the globals rig.sh set (ROOT, MACHINES, GUI, ...).

write_core_json() {
    local l=$1
    local dir="$ROOT/state-$l/herdr-core/rig-$l"
    mkdir -p "$dir"
    printf '{"id":"rig-%s","name":"host-%s"}\n' "$l" "$l" >"$dir/core.json"
}

make_repo() {
    local name=$1 dir="$ROOT/repos/$1"
    mkdir -p "$dir"
    git -C "$dir" init -q -b main
    printf '# %s\n\nNeutral fixture repository for the backlot rig.\n' "$name" >"$dir/README.md"
    git -C "$dir" add README.md
    git -C "$dir" -c user.name=alice -c user.email=alice@example.com \
        commit -q -m "init $name"
}

write_cast() {
    cat >"$ROOT/cast.toml" <<'EOF'
# Neutral fake-runner cast for the backlot rig.
rework_cap = 2
stall_limit_s = 600

[producer]
runner = "fake"
model = "model-p"
effort = "high"
permission_profile = "standard"

[director]
runner = "fake"
model = "model-d"
effort = "high"
permission_profile = "standard"

[writer]
runner = "fake"
model = "model-w"
effort = "medium"
permission_profile = "standard"

[[candidates]]
name = "actor-a"
runner = "fake"
model = "model-a"
effort = "low"
permission_profile = "worktree"
EOF
}

write_via() {
    local l=$1
    mkdir -p "$ROOT/via"
    cat >"$ROOT/via/host-$l.sh" <<EOF
#!/bin/sh
exec env -i "PATH=$ROOT/bin-$l:/usr/bin:/bin" \\
    "HOME=$ROOT/home-$l" "XDG_STATE_HOME=$ROOT/state-$l" \\
    "XDG_CONFIG_HOME=$ROOT/config-$l" "XDG_RUNTIME_DIR=$ROOT/run-$l" \\
    "HC_SESSION=rig-$l" "HC_SOCKET=$ROOT/core-$l.sock" \\
    "BACKLOT_STATE_DIR=$ROOT/backlot-$l" "HERDR_DISABLE_SOUND=1" \\
    sh -c "\$1"
EOF
    chmod 755 "$ROOT/via/host-$l.sh"
}

now_ms() {
    date +%s%3N 2>/dev/null || python3 -c 'import time; print(int(time.time()*1000))'
}

seed_latest() {
    [ -n "$SEED_LATEST" ] || return 0
    mkdir -p "$ROOT/state-a/backlot"
    printf '{"tag":"%s","checked_ms":%s}\n' "$SEED_LATEST" "$(now_ms)" \
        >"$ROOT/state-a/backlot/update-check.json"
}

pick_display() {
    local n=90
    while [ "$n" -lt 140 ]; do
        if [ ! -e "/tmp/.X$n-lock" ] && [ ! -e "/tmp/.X11-unix/X$n" ]; then
            DISPLAY_NAME=":$n"
            return 0
        fi
        n=$((n + 1))
    done
    die 'no free display in :90..:139; pass --display'
}

start_xvfb() {
    local w=${SIZE%x*} h=${SIZE#*x}
    env -i "PATH=/usr/bin:/bin" "HOME=$ROOT/home-a" "TMPDIR=$ROOT/tmp" \
        "LANG=C.UTF-8" "$XVFB" "$DISPLAY_NAME" \
        -screen 0 "${w}x${h}x24" -nolisten tcp -ac \
        >"$ROOT/log-xvfb.log" 2>&1 &
    record_pid xvfb "$!"
    local num=${DISPLAY_NAME#:}
    num=${num%%.*}
    wait_socket "/tmp/.X11-unix/X$num"
}

# GTK would otherwise reach the user's D-Bus and start gvfs/portal FUSE
# mounts inside our private runtime dir. Local GIO and a dead bus keep the
# rig self-contained.
start_kitty() {
    core_env a
    local KENVV=("${ENVV[@]}"
        "DISPLAY=$DISPLAY_NAME"
        "GIO_USE_VFS=local"
        "GVFS_DISABLE_FUSE=1"
        "DBUS_SESSION_BUS_ADDRESS=disabled:"
        "NO_AT_BRIDGE=1"
        "GTK_A11Y=none")
    "${KENVV[@]}" "$KITTY" --listen-on "unix:$ROOT/kitty.sock" \
        --override allow_remote_control=yes \
        --override confirm_os_window_close=0 \
        "$BL" --session rig-a >"$ROOT/log-kitty.log" 2>&1 &
    record_pid kitty "$!"
    wait_socket "$ROOT/kitty.sock"
    local i=0
    while [ "$i" -lt 10 ]; do
        record_runtime_helpers
        sleep 0.1
        i=$((i + 1))
    done
}

# Helpers the GUI stack may spawn asynchronously still name our root in
# their command line; record them so teardown can stop exactly those.
record_runtime_helpers() {
    have pgrep || return 0
    local list p
    list=$({ pgrep -f -- "$ROOT/run-a/gvfs" 2>/dev/null || true
        pgrep -f -- "$ROOT/run-a/doc" 2>/dev/null || true; })
    for p in $list; do
        if ! grep -q " $p\$" "$ROOT/pids" 2>/dev/null; then
            record_pid "helper-$p" "$p"
        fi
    done
}

# A killed portal can leave its FUSE mount behind; unmount before rm.
unmount_runtime() {
    local d
    for d in "$ROOT"/run-*/gvfs "$ROOT"/run-*/doc; do
        [ -d "$d" ] || continue
        fusermount3 -u "$d" 2>/dev/null ||
            fusermount -u "$d" 2>/dev/null ||
            umount "$d" 2>/dev/null || true
    done
}

print_plan() {
    local shown_root=${ROOT:-<auto: mktemp -d ${TMPDIR:-/tmp}/backlot-rig.XXXXXX>}
    local shown_display=${DISPLAY_NAME:-<auto: first free :90..:139>}
    printf 'rig plan\n'
    printf 'root=%s\n' "$shown_root"
    printf 'machines=%s\n' "$MACHINES"
    printf 'display=%s\n' "$shown_display"
    printf 'size=%s\n' "$SIZE"
    printf 'strip=%s\n' "$STRIP_VARS"
    printf 'tool backlot=%s\n' "${BL:-missing}"
    printf 'tool hc=%s\n' "${HC:-missing}"
    printf 'tool core=%s\n' "${CORE:-missing}"
    printf 'tool Xvfb=%s\n' "${XVFB:-missing}"
    printf 'tool kitty=%s\n' "${KITTY:-missing}"
    printf 'tool capture=%s\n' "${CAPTURE:-missing}"
    printf 'repo acme-shop=%s/repos/acme-shop\n' "$shown_root"
    printf 'repo beta-tools=%s/repos/beta-tools\n' "$shown_root"
    printf 'studio machine-a=acme-shop (fake runner)\n'
    if [ "$MACHINES" -ge 2 ]; then
        printf 'studio machine-b=beta-tools (fake runner)\n'
    fi
    if [ "$MACHINES" -ge 3 ]; then
        printf 'machine machine-c=host-c (no backlot on PATH)\n'
    fi
    if [ -n "$SEED_LATEST" ]; then
        printf 'seed=%s/state-a/backlot/update-check.json tag=%s\n' "$shown_root" "$SEED_LATEST"
    fi
    local l
    for l in a b c; do
        if [ "$l" = c ] && [ "$MACHINES" -lt 3 ]; then continue; fi
        if [ "$l" = b ] && [ "$MACHINES" -lt 2 ]; then continue; fi
        printf 'command core-%s: env -i PATH=%s/bin-%s:/usr/bin:/bin HOME=%s/home-%s HC_SESSION=rig-%s HC_SOCKET=%s/core-%s.sock BACKLOT_STATE_DIR=%s/backlot-%s %s serve\n' \
            "$l" "$shown_root" "$l" "$shown_root" "$l" "$l" "$shown_root" "$l" "$shown_root" "$l" "${CORE:-<core>}"
    done
    if [ "$MACHINES" -ge 2 ]; then
        printf 'command machine-b: hc machine add --via %s/via/host-b.sh --name host-b --session rig-b --temp\n' "$shown_root"
    fi
    if [ "$MACHINES" -ge 3 ]; then
        printf 'command machine-c: hc machine add --via %s/via/host-c.sh --name host-c --session rig-c --temp\n' "$shown_root"
    fi
    if [ "$GUI" = 1 ]; then
        printf 'command xvfb: Xvfb %s -screen 0 %sx24 -nolisten tcp -ac\n' "$shown_display" "$SIZE"
        printf 'command kitty: env -i DISPLAY=%s HC_SESSION=rig-a HC_SOCKET=%s/core-a.sock %s --listen-on unix:%s/kitty.sock %s --session rig-a\n' \
            "$shown_display" "$shown_root" "${KITTY:-<kitty>}" "$shown_root" "${BL:-<backlot>}"
    fi
}

start_cores() {
    local l
    for l in a b; do
        if [ "$l" = b ] && [ "$MACHINES" -lt 2 ]; then continue; fi
        core_env "$l"
        "${ENVV[@]}" "$CORE" serve >"$ROOT/log-core-$l.log" 2>&1 &
        record_pid "core-$l" "$!"
        wait_socket "$ROOT/core-$l.sock"
    done
    if [ "$MACHINES" -ge 3 ]; then
        core_env c
        "${ENVV[@]}" "$CORE" serve >"$ROOT/log-core-c.log" 2>&1 &
        record_pid core-c "$!"
        wait_socket "$ROOT/core-c.sock"
    fi
}

start_machines() {
    [ "$MACHINES" -ge 2 ] || return 0
    core_env a
    "${ENVV[@]}" "$HC" machine add --via "$ROOT/via/host-b.sh" \
        --name host-b --session rig-b --temp >/dev/null
    if [ "$MACHINES" -ge 3 ]; then
        "${ENVV[@]}" "$HC" machine add --via "$ROOT/via/host-c.sh" \
            --name host-c --session rig-c --temp >/dev/null
    fi
}

start_studios() {
    core_env a
    "${ENVV[@]}" "$BL" studio new --studio acme-shop --title acme-shop \
        --repo "$ROOT/repos/acme-shop" --cast "$ROOT/cast.toml" >/dev/null
    [ "$MACHINES" -ge 2 ] || return 0
    core_env b
    "${ENVV[@]}" "$BL" studio new --studio beta-tools --title beta-tools \
        --repo "$ROOT/repos/beta-tools" --cast "$ROOT/cast.toml" >/dev/null
}

stop_cores() {
    if [ -z "${ROOT:-}" ] || [ ! -d "$ROOT" ] || [ -z "$CORE" ]; then return 0; fi
    local l
    for l in a b c; do
        [ -S "$ROOT/core-$l.sock" ] || continue
        core_env "$l"
        "${ENVV[@]}" "$CORE" --json server stop >/dev/null 2>&1 || true
    done
}

stop_pids() {
    [ -n "${ROOT:-}" ] && [ -f "$ROOT/pids" ] || return 0
    local -a names=() pids=()
    local n p
    while read -r n p; do
        [ -n "${p:-}" ] || continue
        names+=("$n")
        pids+=("$p")
    done <"$ROOT/pids"
    local i
    for ((i = ${#pids[@]} - 1; i >= 0; i--)); do
        if pid_alive "${pids[i]}"; then
            kill -TERM "${pids[i]}" 2>/dev/null || true
        fi
    done
    local t=0 alive
    while [ "$t" -lt 30 ]; do
        alive=0
        for p in "${pids[@]}"; do
            if pid_alive "$p"; then alive=1; fi
        done
        if [ "$alive" -eq 0 ]; then break; fi
        sleep 0.1
        t=$((t + 1))
    done
    for p in "${pids[@]}"; do
        if pid_alive "$p"; then kill -KILL "$p" 2>/dev/null || true; fi
    done
    for i in "${!pids[@]}"; do
        if pid_alive "${pids[i]}"; then
            printf 'rig: recorded process still alive: %s %s\n' \
                "${names[i]}" "${pids[i]}" >&2
            return 1
        fi
    done
}
