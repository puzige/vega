#!/bin/sh
# Compatibility entry point; see docs/vega-issue-107-test-workflow.md V4/V5.
set -eu
exec python3 "$(dirname "$0")/cargo-coordinate.py" "$@"
