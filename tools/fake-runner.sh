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
    # A real two-option picker, one byte at a time: Escape starts a
    # cursor key (Down moves the cursor onto the trust option, Up
    # back to "No, exit"), CR/LF accepts whatever the cursor is on.
    # Each frame holds a moment so a watcher can see the cursor move.
    cursor=no
    while :; do
        printf '\033[2J\033[H'
        printf 'Quick safety check\nDo you trust the files in this folder?\n\n'
        if [ "$cursor" = no ]; then
            printf '\342\235\257 No, exit\n  Yes, I trust this folder\n'
        else
            printf '  No, exit\n\342\235\257 Yes, I trust this folder\n'
        fi
        sleep 1
        b=$(dd bs=1 count=1 2>/dev/null | od -An -tx1 | tr -d ' \n')
        case "$b" in
            1b)
                seq=$(dd bs=1 count=2 2>/dev/null | od -An -tx1 | tr -d ' \n')
                case "$seq" in
                    5b42 | 4f42) cursor=yes ;;
                    5b41 | 4f41) cursor=no ;;
                esac
                ;;
            0d | 0a)
                [ "$cursor" = yes ] || cursor=declined
                break
                ;;
            "") cursor=eof; break ;;
        esac
    done
    [ -n "$saved" ] && stty "$saved" 2>/dev/null
    printf '\033[2J\033[H'
    if [ "$cursor" != yes ]; then
        echo "fake runner: trust dialog not answered (cursor=$cursor)"
        cat >/dev/null
        exit 0
    fi
fi
echo "fake runner: ${BACKLOT_ROLE:-role}${BACKLOT_TASK:+ task $BACKLOT_TASK} of ${BACKLOT_STUDIO:-studio}"
cat >/dev/null
