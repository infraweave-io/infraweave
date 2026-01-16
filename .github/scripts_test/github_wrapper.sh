#!/bin/bash
set -euo pipefail

# Shared wrapper script for GitHub Actions scripts
# Creates temporary GITHUB_OUTPUT and GITHUB_STEP_SUMMARY files, executes the script, and prints the output
#
# Usage: github_wrapper.sh <script_path> [script_args...]

if [ $# -lt 1 ]; then
    echo "Usage: $0 <script_path> [script_args...]" >&2
    exit 1
fi

SCRIPT_TO_RUN="$1"
shift  # Remove first argument, rest are passed to the script

# Create temporary files for GITHUB_OUTPUT and GITHUB_STEP_SUMMARY
GITHUB_OUTPUT=$(mktemp)
export GITHUB_OUTPUT
GITHUB_STEP_SUMMARY=$(mktemp)
export GITHUB_STEP_SUMMARY

# Cleanup function
cleanup() {
    rm -f "$GITHUB_OUTPUT" "$GITHUB_STEP_SUMMARY"
}
trap cleanup EXIT

# Execute the original script
echo "Executing: $SCRIPT_TO_RUN"
if bash "$SCRIPT_TO_RUN" "$@"; then
    EXIT_CODE=0
else
    EXIT_CODE=$?
fi

# Print the content of GITHUB_OUTPUT (if any)
echo ""
echo "=== GITHUB_OUTPUT content ==="
if [ -s "$GITHUB_OUTPUT" ]; then
    cat "$GITHUB_OUTPUT"
else
    echo "(empty or not set)"
fi
echo "============================="

# Print the content of GITHUB_STEP_SUMMARY (if any)
echo ""
echo "=== GITHUB_STEP_SUMMARY content ==="
if [ -s "$GITHUB_STEP_SUMMARY" ]; then
    cat "$GITHUB_STEP_SUMMARY"
else
    echo "(empty or not set)"
fi
echo "==================================="
echo ""
echo "Script exit code: $EXIT_CODE"

# Exit with the same code as the original script
exit $EXIT_CODE
