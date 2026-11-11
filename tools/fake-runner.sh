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
# `fake-approved` marker and returns the runner to `waiting`. A
# byte that is none of those begins a line — it is read to its
# newline: a `hook:` line plays as usual and the prompt stays up
# (the line's newline is not a key), any other line is dropped.
# `prompt:<text>` sets the prompt's own text; the next `fake-work:
# blocked` — and each chained one — echoes it as `fake-prompt:
# <text>` so two prompts differ on screen.
# `work:blocked:<n>` chains <n> prompts back to back: the first
# <n>-1 approvals print a new `fake-work: blocked` — the next
# prompt already up, no unblocked screen in between — and only the
# last one returns to `waiting`. `hook:<event>[:<arg>]` pipes a
# Claude-shaped hook JSON to `sh -c "$BACKLOT_HOOK_CMD"` — the fake
# plays the S9f hooks so the stage's signal path is exercisable
# without a real Claude.
state=waiting
prompts=0
ptext=
saved=$(stty -g 2>/dev/null)
# Pipe a `hook:<event>[:<arg>]` line to the hook command as a
# Claude-shaped JSON — the arg is the Bash command for a
# PermissionRequest, the type for a Notification, the error for a
# StopFailure.
play_hook() {
    ev="${1#hook:}"
    arg=
    case "$ev" in
        *:*)
            arg="${ev#*:}"
            ev="${ev%%:*}"
            ;;
    esac
    jarg=$(printf '%s' "$arg" | sed -e 's/\\/\\\\/g' -e 's/"/\\"/g')
    extra=
    case "$ev" in
        PermissionRequest)
            extra=',"tool_name":"Bash","tool_input":{"command":"'"$jarg"'"}'
            ;;
        Notification)
            extra=',"notification_type":"'"$jarg"'"'
            ;;
        StopFailure)
            extra=',"error":"'"$jarg"'"'
            ;;
    esac
    printf '{"session_id":"%s","hook_event_name":"%s"%s}\n' \
        "${FAKE_SESSION:-fake-unknown}" "$ev" "$extra" |
        sh -c "${BACKLOT_HOOK_CMD:-:}"
}
while :; do
    if [ "$state" = blocked ]; then
        stty -icanon -echo min 1 time 0 2>/dev/null
        b=$(dd bs=1 count=1 2>/dev/null | od -An -tx1 | tr -d ' \n')
        case "$b" in
            0d | 0a | 1b)
                if [ "$b" = 1b ]; then key=esc; else key=enter; fi
                prompts=$((prompts - 1))
                if [ "$prompts" -gt 0 ]; then
                    printf 'fake-approved: %s\nfake-work: blocked\n' "$key"
                    [ -n "$ptext" ] && printf 'fake-prompt: %s\n' "$ptext"
                else
                    [ -n "$saved" ] && stty "$saved" 2>/dev/null
                    printf 'fake-approved: %s\nfake-work: waiting\n' "$key"
                    state=waiting
                fi
                ;;
            "") break ;;
            *)
                # Not a key byte: it begins a line — read to its
                # newline. A `hook:` line plays as usual and the
                # prompt stays up — the line's newline is not a
                # key; any other line is dropped.
                first=$(printf "$(printf '\\%03o' "0x$b")")
                IFS= read -r rest || break
                case "$first$rest" in
                    hook:*) play_hook "$first$rest" ;;
                esac
                ;;
        esac
    else
        IFS= read -r line || break
        case "$line" in
            hook:*) play_hook "$line" ;;
            work:blocked:*)
                state=blocked
                prompts="${line##*:}"
                printf 'fake-work: blocked\n'
                [ -n "$ptext" ] && printf 'fake-prompt: %s\n' "$ptext"
                ;;
            work:*)
                state="${line#work:}"
                prompts=1
                printf 'fake-work: %s\n' "$state"
                [ "$state" = blocked ] && [ -n "$ptext" ] &&
                    printf 'fake-prompt: %s\n' "$ptext"
                ;;
            prompt:*)
                ptext="${line#prompt:}"
                ;;
            "")
                ;; # a send's own trailing CR, not a message
            *)
                [ -n "${BACKLOT_STATE_DIR:-}" ] &&
                    printf '%s\n' "$line" \
                        >>"$BACKLOT_STATE_DIR/fake-inbox-${BACKLOT_ROLE:-role}"
                ;;
        esac
    fi
done
