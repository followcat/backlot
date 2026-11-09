#!/bin/sh
# The fake runner: announce the role, then stay alive consuming input
# so the hc terminal's program keeps running until killed. With
# --version it prints a version and exits, like a real runner. With
# FAKE_MODEL=trust it first plays Claude Code's folder-trust dialog
# as the real TUI does: a cursor that starts on "No, exit", one key
# at a time — Down moves the cursor onto "Yes, I trust this folder",
# Enter accepts, and only then does the runner announce itself.
if [ "$1" = "--version" ]; then
    echo "fake-runner 1.0"
    exit 0
fi
if [ "${FAKE_MODEL:-}" = "trust" ]; then
    saved=$(stty -g 2>/dev/null)
    stty -icanon -echo 2>/dev/null
    # Draw 1: the picker cursor sits on "No, exit".
    printf '\033[2J\033[H'
    printf 'Quick safety check\nDo you trust the files in this folder?\n\n'
    printf '\342\235\257 No, exit\n  Yes, I trust this folder\n'
    # Down is ESC [ B or ESC O B — one key, three bytes.
    keys=$(dd bs=1 count=3 2>/dev/null | od -An -tx1 | tr -d ' \n')
    case "$keys" in
        1b5b42 | 1b4f42) ok=yes ;;
        *) ok= ;;
    esac
    if [ -n "$ok" ]; then
        # Draw 2: the cursor moved onto the trust option.
        printf '\033[2J\033[H'
        printf 'Quick safety check\nDo you trust the files in this folder?\n\n'
        printf '  No, exit\n\342\235\257 Yes, I trust this folder\n'
        # Enter is CR or LF — one key, one byte.
        keys=$(dd bs=1 count=1 2>/dev/null | od -An -tx1 | tr -d ' \n')
        case "$keys" in
            0d | 0a) ;;
            *) ok= ;;
        esac
    fi
    [ -n "$saved" ] && stty "$saved" 2>/dev/null
    printf '\033[2J\033[H'
    if [ -z "$ok" ]; then
        echo "fake runner: trust dialog never answered"
        cat >/dev/null
        exit 0
    fi
fi
echo "fake runner: ${BACKLOT_ROLE:-role}${BACKLOT_TASK:+ task $BACKLOT_TASK} of ${BACKLOT_STUDIO:-studio}"
cat >/dev/null
