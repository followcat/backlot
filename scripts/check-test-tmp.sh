#!/bin/sh
# Run the workspace tests with TMPDIR pointed at a symlink to a fresh,
# empty directory (matching macOS's /var -> /private/var TMPDIR unless
# BACKLOT_TEST_TMP_NOLINK=1) and fail when the tests leave anything behind.
# A test core's lock files are not leftovers.
#
# Extra arguments are passed to `cargo test`.
set -u

base=${TMPDIR:-/tmp}
real=$(mktemp -d "$base/backlot-testtmp.XXXXXX") || exit 1
link_dir=
if [ "${BACKLOT_TEST_TMP_NOLINK:-}" != "1" ]; then
    link_dir=$(mktemp -d "$base/blt.XXXXXX") || { rm -rf "$real"; exit 1; }
    test_tmp="$link_dir/t"
    if ! ln -s "$real" "$test_tmp"; then
        rm -rf "$real" "$link_dir"
        exit 1
    fi
else
    test_tmp=$real
fi
trap 'rm -rf "$real" ${link_dir:+"$link_dir"}' EXIT

status=0
TMPDIR="$test_tmp" cargo test --workspace "$@" || status=$?

left=$(find "$real" -mindepth 1 -maxdepth 1 ! -name '*.lock' | sort)
if [ -n "$left" ]; then
    echo "tests left files in TMPDIR:" >&2
    printf '%s\n' "$left" >&2
    trap - EXIT
    [ -z "$link_dir" ] || rm -rf "$link_dir"
    echo "the directory is kept for inspection: $real" >&2
    exit 1
fi

exit "$status"
