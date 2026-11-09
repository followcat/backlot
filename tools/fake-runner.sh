#!/bin/sh
# The fake runner: announce the role, then stay alive consuming input
# so the hc terminal's program keeps running until killed. With
# --version it prints a version and exits, like a real runner. With
# FAKE_MODEL=trust it first shows a folder-trust dialog and waits for
# Down then Enter, the exchange the stage answers for Claude Code.
if [ "$1" = "--version" ]; then
    echo "fake-runner 1.0"
    exit 0
fi
if [ "${FAKE_MODEL:-}" = "trust" ]; then
    echo "Quick safety check"
    echo "Yes, I trust this folder"
    # Wait for Down (\033[B or \033OB) then Enter (\r or \n).
    keys=$(dd bs=1 count=4 2>/dev/null | od -An -tx1 | tr -d ' \n')
    case "$keys" in
        1b5b420d | 1b5b420a | 1b4f420d | 1b4f420a) ;;
        *)
            echo "fake runner: trust dialog never answered"
            cat >/dev/null
            exit 0
            ;;
    esac
fi
echo "fake runner: ${BACKLOT_ROLE:-role}${BACKLOT_TASK:+ task $BACKLOT_TASK} of ${BACKLOT_STUDIO:-studio}"
cat >/dev/null
