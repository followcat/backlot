#!/bin/sh
# The S2 fake runner: announce the role, then stay alive consuming
# input so the hc terminal's program keeps running until killed.
echo "fake runner: ${BACKLOT_ROLE:-role}${BACKLOT_TASK:+ task $BACKLOT_TASK} of ${BACKLOT_STUDIO:-studio}"
cat >/dev/null
