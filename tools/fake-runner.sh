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
    stty -icanon -echo min 0 time 0 2>/dev/null
    # The first paint, then the measured unsafe second: any byte
    # that arrives inside it is judged early — drained, logged for
    # the test, never queued. An early Enter kills the runner with
    # the dialog still up; an early Down is just lost.
    printf '\033[2J\033[H'
    printf 'Quick safety check\nDo you trust the files in this folder?\n\n'
    printf '\342\235\257 No, exit\n  Yes, I trust this folder\n'
    sleep 1
    early=$(dd bs=1 count=64 2>/dev/null | od -An -tx1 | tr -d ' \n')
    if [ -n "$early" ]; then
        [ -n "${BACKLOT_STATE_DIR:-}" ] &&
            printf '%s\n' "$early" >>"$BACKLOT_STATE_DIR/fake-early-keys"
        case "$early" in
            *0d* | *0a*) exit 1 ;;
        esac
    fi
    stty min 1 time 0 2>/dev/null
    # Past the unsafe second the picker is robust: one byte at a
    # time, Escape starts a cursor key (Down moves the cursor onto
    # the trust option, Up back to "No, exit"), CR/LF accepts
    # whatever the cursor is on.
    cursor=no
    while :; do
        printf '\033[2J\033[H'
        printf 'Quick safety check\nDo you trust the files in this folder?\n\n'
        if [ "$cursor" = no ]; then
            printf '\342\235\257 No, exit\n  Yes, I trust this folder\n'
        else
            printf '  No, exit\n\342\235\257 Yes, I trust this folder\n'
        fi
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
# The input loop. `work:<state>` echoes `fake-work: <state>` for the
# stage's work sample and becomes the runner's state; every other
# whole line — a `[backlot]` message, a prompt — lands in the
# role's inbox under the state dir when one is set. While `blocked`
# the runner reads single bytes like a real permission prompt: CR
# or LF is the allow key, ESC the deny, and either prints a
# `fake-approved` marker and returns the runner to `waiting`.
state=waiting
saved=$(stty -g 2>/dev/null)
while :; do
    if [ "$state" = blocked ]; then
        stty -icanon -echo min 1 time 0 2>/dev/null
        b=$(dd bs=1 count=1 2>/dev/null | od -An -tx1 | tr -d ' \n')
        case "$b" in
            0d | 0a)
                [ -n "$saved" ] && stty "$saved" 2>/dev/null
                printf 'fake-approved: enter\nfake-work: waiting\n'
                state=waiting
                ;;
            1b)
                [ -n "$saved" ] && stty "$saved" 2>/dev/null
                printf 'fake-approved: esc\nfake-work: waiting\n'
                state=waiting
                ;;
            "") break ;;
        esac
    else
        IFS= read -r line || break
        case "$line" in
            work:*)
                state="${line#work:}"
                printf 'fake-work: %s\n' "$state"
                ;;
            "")
                ;; # a send's own trailing CR, not a message
            *)
                [ -n "${BACKLOT_STATE_DIR:-}" ] &&
                    printf '%s\n' "$line" \
                        >>"$BACKLOT_STATE_DIR/fake-inbox-${BACKLOT_ROLE:-role}${BACKLOT_TASK:+-$BACKLOT_TASK}"
                ;;
        esac
    fi
done
