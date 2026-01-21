#!/bin/bash
set -euo pipefail

# Wrapper script for wheels_validate-targets.sh
# Uses the shared github_wrapper.sh to create a temporary GITHUB_OUTPUT file,
# execute the script, and print the output

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/../.." && pwd)"
ORIGINAL_SCRIPT="$REPO_ROOT/.github/scripts/wheels_validate-targets.sh"
WRAPPER_SCRIPT="$SCRIPT_DIR/github_wrapper.sh"

# Execute using the shared wrapper
exec "$WRAPPER_SCRIPT" "$ORIGINAL_SCRIPT"

