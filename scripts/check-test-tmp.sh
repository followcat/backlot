#!/bin/sh
# Run the workspace tests with TMPDIR pointed at a fresh, empty directory
# and fail when the tests leave anything behind. A test core's lock files
# are not leftovers.
#
# Extra arguments are passed to `cargo test`.
set -u

root=$(mktemp -d "${TMPDIR:-/tmp}/backlot-testtmp.XXXXXX") || exit 1
trap 'rm -rf "$root"' EXIT

status=0
TMPDIR="$root" cargo test --workspace "$@" || status=$?

left=$(find "$root" -mindepth 1 -maxdepth 1 ! -name '*.lock' | sort)
if [ -n "$left" ]; then
    echo "tests left files in TMPDIR:" >&2
    printf '%s\n' "$left" >&2
    trap - EXIT
    echo "the directory is kept for inspection: $root" >&2
    exit 1
fi

exit "$status"
