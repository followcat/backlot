#!/bin/sh
# Prints how many hc-tui commits and releases backlot is behind the
# hc-tui base recorded in docs/compatibility.md.
set -eu

root=$(git rev-parse --show-toplevel)
cd "$root"

base=$(sed -n 's/.*hc-tui base: v[^ ]* (`\([0-9a-f]\{40\}\)`).*/\1/p' docs/compatibility.md | head -n 1)
if [ -z "$base" ]; then
    echo "hc-tui-behind.sh: missing hc-tui base sha in docs/compatibility.md" >&2
    exit 1
fi

# Fetch upstream tags into a separate namespace so backlot's own v* tags
# (e.g. v0.1.0) are never rejected as clobbers.
git fetch -q --no-tags upstream \
    +refs/heads/main:refs/remotes/upstream/main \
    '+refs/tags/v*:refs/upstream-tags/v*'
commits=$(git rev-list --count "${base}..upstream/main")
tags=$(git for-each-ref \
    --merged=upstream/main \
    --no-merged="$base" \
    --sort=v:refname \
    --format='%(refname:lstrip=2)' \
    'refs/upstream-tags/v*')

echo "hc-tui base: $base"
echo "commits behind: $commits"
if [ -z "$tags" ]; then
    echo "releases behind: 0"
else
    count=$(printf '%s\n' "$tags" | wc -l | tr -d ' ')
    echo "releases behind: $count"
    printf '%s\n' "$tags"
fi
