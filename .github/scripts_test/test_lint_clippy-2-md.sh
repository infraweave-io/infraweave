#!/bin/bash
set -euo pipefail

# Test script for lint_clippy-2-md.sh
# Uses the shared github_wrapper.sh to create a temporary GITHUB_STEP_SUMMARY file,
# execute the script, and print the output.
# Runs cargo clippy in the repo; script exit code reflects clippy result.
#
# Usage: test_lint_clippy-2-md.sh [ALL|CRATE_NAME]
#   No args  - show menu to select one crate or all
#   ALL      - run clippy on every workspace crate
#   CRATE    - run clippy only on the named crate

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/../.." && pwd)"
ORIGINAL_SCRIPT="$REPO_ROOT/.github/scripts/lint_clippy-2-md.sh"
WRAPPER_SCRIPT="$SCRIPT_DIR/github_wrapper.sh"
# Get workspace member package names from cargo metadata (preserves workspace order)
# Must run from repo root so cargo finds the workspace
members=()
while IFS= read -r line; do
  members+=( "$line" )
done < <(cd "$REPO_ROOT" && cargo metadata --no-deps --format-version 1 2>/dev/null | jq -r '.workspace_members[] as $id | .packages[] | select(.id == $id) | .name')

if [ ${#members[@]} -eq 0 ]; then
  echo "Error: No workspace members found (run from repo root or check cargo metadata)" >&2
  exit 1
fi

TARGET="${1:-}"

if [ -z "$TARGET" ]; then
  echo "Select crate to run clippy on:"
  echo "  0) all (every workspace crate)"
  i=1
  for m in "${members[@]}"; do
    echo "  $i) $m"
    ((i++)) || true
  done
  echo ""
  read -r -p "Choice [0-${#members[@]}]: " choice
  if [[ "$choice" == "0" ]]; then
    TARGET="ALL"
  elif [[ "$choice" =~ ^[0-9]+$ ]] && [ "$choice" -ge 1 ] && [ "$choice" -le ${#members[@]} ]; then
    TARGET="${members[$((choice - 1))]}"
  else
    echo "Invalid choice. Use 0 for all or 1-${#members[@]} for a crate." >&2
    exit 1
  fi
fi

# Execute using the shared wrapper (must run from repo root for cargo)
cd "$REPO_ROOT"
exec "$WRAPPER_SCRIPT" "$ORIGINAL_SCRIPT" "$TARGET"
