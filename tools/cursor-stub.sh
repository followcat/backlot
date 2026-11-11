#!/bin/sh
# The cursor-agent stand-in for stage tests: prints a cursor-shaped
# idle screen with the CURSOR_* env it was given, creates the chat
# dir the stage's session discovery looks for —
# $CURSOR_CONFIG_DIR/chats/<md5(cwd)>/<uuid>/meta.json — then stays
# alive consuming input so the hc terminal's program keeps running
# until killed. It never contacts the real runner.
if [ "$1" = "--version" ]; then
    echo "cursor-stub 2026.11.05"
    exit 0
fi
uuid="11111111-2222-3333-4444-555555555555"
hash=$(printf %s "$PWD" | md5sum | cut -d' ' -f1)
dir="${CURSOR_CONFIG_DIR:?}/chats/$hash/$uuid"
mkdir -p "$dir"
printf '{"createdAtMs":%s,"cwd":"%s"}\n' "$(date +%s%3N)" "$PWD" >"$dir/meta.json"
printf '  Cursor Agent\n'
printf '  v2026.11.05-stub\n'
printf '  CURSOR_CONFIG_DIR=%s\n' "$CURSOR_CONFIG_DIR"
printf '  CURSOR_DATA_DIR=%s\n' "$CURSOR_DATA_DIR"
printf '  CURSOR_SANDBOX_POLICY_DIR=%s\n' "$CURSOR_SANDBOX_POLICY_DIR"
printf '  → Plan, search, build anything\n'
printf '  Composer 2.5\n'
printf '  %s · main\n' "$PWD"
# Stay alive consuming input, like the real TUI at its idle composer.
while IFS= read -r _; do :; done
