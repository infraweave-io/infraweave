#!/bin/bash
set -euo pipefail

# Run cargo clippy (all crates or a single crate), convert JSON to markdown, write to GITHUB_STEP_SUMMARY.
# Exits with non-zero if any crate reported warnings/errors.
#
# Usage: lint_clippy-2-md.sh <ALL|CRATE_NAME> [path/to/Cargo.toml]
#   ALL          - run clippy on every workspace member
#   CRATE_NAME   - run clippy only on the named crate

if [ $# -lt 1 ]; then
  echo "Usage: $0 <ALL|CRATE_NAME> [path/to/Cargo.toml]" >&2
  echo "  ALL          run on all workspace crates" >&2
  echo "  CRATE_NAME   run only on the named crate" >&2
  exit 1
fi

TARGET="$1"
root_cargo="${2:-./Cargo.toml}"
if [ ! -f "$root_cargo" ]; then
  echo "Error: Cargo.toml not found at $root_cargo" >&2
  exit 1
fi
SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
JQ_SCRIPT="${SCRIPT_DIR}/jq-to-markdown.jq"
WORKSPACE_ROOT="$(cd "$(dirname "$root_cargo")" && pwd)"
cd "$WORKSPACE_ROOT"

# Get workspace member package names from cargo metadata (preserves workspace order)
members=()
while IFS= read -r line; do
  members+=( "$line" )
done < <(cargo metadata --no-deps --format-version 1 2>/dev/null | jq -r '.workspace_members[] as $id | .packages[] | select(.id == $id) | .name')

if [ ${#members[@]} -eq 0 ]; then
  echo "Error: No workspace members found (run from workspace root or check cargo metadata)" >&2
  exit 1
fi

# Build list of crates to run: all members or just the named crate
if [ "$TARGET" = "ALL" ] || [ "$TARGET" = "all" ]; then
  crates=( "${members[@]}" )
else
  crates=()
  for m in "${members[@]}"; do
    if [ "$m" = "$TARGET" ]; then
      crates=( "$TARGET" )
      break
    fi
  done
  if [ ${#crates[@]} -eq 0 ]; then
    echo "Error: Crate '$TARGET' is not a workspace member. Members: ${members[*]}" >&2
    exit 1
  fi
fi

clippy_exit=0
tmpout=$(mktemp)
trap 'rm -f "$tmpout"' EXIT

echo -e "## Clippy results\n" >> "$GITHUB_STEP_SUMMARY"

for pkg in "${crates[@]}"; do
  echo "Running clippy on ${pkg}..."
  echo -e "### Crate: \`${pkg}\`\n" >> "$GITHUB_STEP_SUMMARY"
  set +e
  cargo clippy -p "$pkg" -q --message-format json-diagnostic-rendered-ansi --no-deps --all-targets > "$tmpout"
  pkg_exit=$?
  if [ "$pkg_exit" -ne 0 ]; then
    cat "$tmpout" | jq -n -r 'inputs | select(.reason == "compiler-message" and .message.level == "error") | .message.rendered '
    [ "$pkg_exit" -ne 0 ] && clippy_exit="$pkg_exit"
    echo -e "**Status:** ❌ Clippy reported diagnostics (exit code ${pkg_exit})\n" >> "$GITHUB_STEP_SUMMARY"
  else
    echo -e "**Status:** ✅ No errors\n" >> "$GITHUB_STEP_SUMMARY"
  fi
  echo "Clippy exit code: ${pkg_exit}"
  set -e

  if [ "$pkg_exit" -ne 0 ]; then
    cat "$tmpout" | jq -rn -f "$JQ_SCRIPT" >> "$GITHUB_STEP_SUMMARY" 2>/dev/null || true
    echo -e "\n" >> "$GITHUB_STEP_SUMMARY"
  fi
done

exit "$clippy_exit"
