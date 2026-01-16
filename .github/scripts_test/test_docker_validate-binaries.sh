#!/bin/bash
set -euo pipefail

# Test script for docker_validate-binaries.sh
# Uses the shared github_wrapper.sh to create a temporary GITHUB_STEP_SUMMARY file,
# execute the script, and print the output
# The script will automatically load data from files in .github/vars

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/../.." && pwd)"
ORIGINAL_SCRIPT="$REPO_ROOT/.github/scripts/docker_validate-binaries.sh"
WRAPPER_SCRIPT="$SCRIPT_DIR/github_wrapper.sh"

# Execute using the shared wrapper
# The script will read from .github/vars/default.docker.json, 
# .github/vars/default.targets.json, and .github/vars/default.binaries.json
exec "$WRAPPER_SCRIPT" "$ORIGINAL_SCRIPT"

