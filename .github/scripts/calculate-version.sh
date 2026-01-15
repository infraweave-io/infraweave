#!/bin/bash
set -euo pipefail

# Calculate version and count commits
# This script determines the semantic version based on:
# - Last git tag
# - Commits since last tag (analyzing for breaking changes, features, patches)
# - Context (PR, branch, release status)

echo "::group::🔍 Finding last tag"
# Fetch tags (optimized: no file contents, only commit metadata)
echo "📥 Fetching tags..."
git fetch --tags --filter=blob:none 2>/dev/null || true

# Get the latest tag, or use v0.0.0 if no tags exist
LAST_TAG=$(git describe --tags --abbrev=0 2>/dev/null || echo "v0.0.0")
if [ "$LAST_TAG" = "v0.0.0" ]; then
  echo "⚠️  No tags found in repository, using v0.0.0 as baseline"
else
  echo "✅ Found last tag: $LAST_TAG"
  
  # Ensure tag commit exists (critical for range operations)
  echo "📥 Ensuring tag commit is available..."
  git fetch origin --filter=blob:none "${LAST_TAG}" 2>/dev/null || true
  
  # Fetch commit history between tag and HEAD (no file contents)
  # This optimization works with shallow clones and blob filters
  echo "📥 Fetching commit history between $LAST_TAG and HEAD (messages only)..."
  if ! git fetch origin --filter=blob:none "${LAST_TAG}..HEAD" 2>/dev/null; then
    # Fallback: fetch more history if range fetch fails (e.g., shallow clone limitations)
    echo "⚠️  Range fetch failed, fetching more history..."
    # Fetch a reasonable amount of history (last 200 commits) to ensure we can resolve ranges
    git fetch origin --filter=blob:none --depth=200 HEAD 2>/dev/null || true
    # Try range fetch again
    git fetch origin --filter=blob:none "${LAST_TAG}..HEAD" 2>/dev/null || true
  fi
fi
echo "::endgroup::"

echo "::group::📊 Analyzing commits"
# Get all commit messages since last tag (subject and body)
if [ "$LAST_TAG" = "v0.0.0" ]; then
  # If no tags exist, get all commits
  echo "📝 Retrieving all commits (no previous tag found)"
  COMMITS=$(git log --pretty=format:"%s%n%b" --no-merges)
else
  # Get commits since last tag
  echo "📝 Retrieving commits since $LAST_TAG"
  COMMITS=$(git log ${LAST_TAG}..HEAD --pretty=format:"%s%n%b" --no-merges)
fi

# Count commits (handle empty case)
# Use git rev-list for accurate commit counting
if [ "$LAST_TAG" = "v0.0.0" ]; then
  # If no tags exist, count all commits
  COMMIT_COUNT=$(git rev-list --count HEAD --no-merges 2>/dev/null || echo "0")
else
  # Count commits since last tag
  COMMIT_COUNT=$(git rev-list --count ${LAST_TAG}..HEAD --no-merges 2>/dev/null || echo "0")
fi

echo "📈 Total commits since last tag: $COMMIT_COUNT"
echo "::endgroup::"

echo "::group::🏷️  Extracting base version"
# Extract version from last tag (remove 'v' prefix if present)
if [[ "$LAST_TAG" =~ ^v?([0-9]+)\.([0-9]+)\.([0-9]+) ]]; then
  MAJOR="${BASH_REMATCH[1]}"
  MINOR="${BASH_REMATCH[2]}"
  PATCH="${BASH_REMATCH[3]}"
  echo "✅ Parsed version from tag: $LAST_TAG → ${MAJOR}.${MINOR}.${PATCH}"
else
  # Default to 0.0.0 if tag format is unexpected
  MAJOR=0
  MINOR=0
  PATCH=0
  echo "⚠️  Tag format unexpected, defaulting to 0.0.0"
fi
echo "::endgroup::"

# If no commits, return the same version
if [ "$COMMIT_COUNT" -eq 0 ]; then
  NEW_VERSION="${MAJOR}.${MINOR}.${PATCH}"
  echo "commit_count=$COMMIT_COUNT" >> $GITHUB_OUTPUT
  echo "version=$NEW_VERSION" >> $GITHUB_OUTPUT
  echo ""
  echo "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"
  echo "📌 VERSION CALCULATION SUMMARY"
  echo "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"
  echo "  Last tag:        $LAST_TAG"
  echo "  Commits since:   $COMMIT_COUNT"
  echo "  Base version:    ${MAJOR}.${MINOR}.${PATCH}"
  echo "  Final version:   $NEW_VERSION"
  echo ""
  echo "  💡 Reason: No commits since last tag, version unchanged"
  echo "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"
  
  # Write to GitHub step summary
  {
    echo "## 📌 VERSION CALCULATION SUMMARY"
    echo ""
    echo "| Field | Value |"
    echo "|-------|-------|"
    echo "| Last tag | \`$LAST_TAG\` |"
    echo "| Commits since | $COMMIT_COUNT |"
    echo "| Base version | \`${MAJOR}.${MINOR}.${PATCH}\` |"
    echo "| Final version | \`$NEW_VERSION\` |"
    echo ""
    echo "💡 **Reason:** No commits since last tag, version unchanged"
  } >> "$GITHUB_STEP_SUMMARY"
  
  exit 0
fi

echo "::group::🔎 Analyzing commit types"
# Check for breaking changes
# Conventional commits: "BREAKING CHANGE:" or "!" in type/scope (e.g., "feat!: ..." or "feat(scope)!: ...")
# Match patterns like: "feat!:", "feat(scope)!:", or "BREAKING CHANGE:" anywhere in the message
# Use a temporary file to avoid issues with large COMMITS variable and ensure compatibility with older bash
TEMP_COMMITS=$(mktemp)
trap "rm -f '$TEMP_COMMITS'" EXIT

# Write commits to temp file, handling empty case
if [ -n "$COMMITS" ]; then
  printf '%s\n' "$COMMITS" > "$TEMP_COMMITS"
else
  touch "$TEMP_COMMITS"
fi

# Check for breaking changes - split pattern for better compatibility
# Pattern 1: "BREAKING CHANGE:" anywhere in the message
# Pattern 2: "type!:" or "type(scope)!:" at start of line
HAS_BREAKING=0
if [ -s "$TEMP_COMMITS" ]; then
  # Count BREAKING CHANGE: occurrences (grep -c returns count with newline)
  # Strip all whitespace to prevent arithmetic errors, default to 0 on failure
  BREAKING_COUNT=$(grep -ciE "BREAKING CHANGE:" "$TEMP_COMMITS" 2>/dev/null | tr -d '[:space:]' || echo "0")
  # Ensure it's numeric and not empty (case statement is more portable)
  case "$BREAKING_COUNT" in
    ''|*[!0-9]*) BREAKING_COUNT=0 ;;
  esac
  
  # Count type!: or type(scope)!: patterns at start of line
  TYPE_BREAKING_COUNT=$(grep -ciE "^[a-z]+(\([^)]+\))?!:" "$TEMP_COMMITS" 2>/dev/null | tr -d '[:space:]' || echo "0")
  # Ensure it's numeric and not empty
  case "$TYPE_BREAKING_COUNT" in
    ''|*[!0-9]*) TYPE_BREAKING_COUNT=0 ;;
  esac
  
  HAS_BREAKING=$((BREAKING_COUNT + TYPE_BREAKING_COUNT))
fi

# Check for features (feat at start of line, case insensitive)
HAS_FEAT=0
if [ -s "$TEMP_COMMITS" ]; then
  HAS_FEAT=$(grep -ciE "^feat" "$TEMP_COMMITS" 2>/dev/null | tr -d '[:space:]' || echo "0")
  # Ensure it's numeric and not empty
  case "$HAS_FEAT" in
    ''|*[!0-9]*) HAS_FEAT=0 ;;
  esac
fi

# Clean up temp file
rm -f "$TEMP_COMMITS"
trap - EXIT

echo "  Breaking changes: $HAS_BREAKING"
echo "  Features:         $HAS_FEAT"
echo "  Other commits:    $((COMMIT_COUNT - HAS_BREAKING - HAS_FEAT))"
echo "::endgroup::"

echo "::group::📈 Calculating version increment"
# Calculate new version based on rules
VERSION_REASON=""
if [ "$HAS_BREAKING" -gt 0 ]; then
  # Breaking change: increment major, reset minor and patch
  NEW_MAJOR=$((MAJOR + 1))
  NEW_MINOR=0
  NEW_PATCH=0
  VERSION_REASON="Breaking change detected ($HAS_BREAKING breaking change(s))"
  echo "🔴 $VERSION_REASON"
  echo "   Incrementing MAJOR version: ${MAJOR} → ${NEW_MAJOR}"
  echo "   Resetting MINOR and PATCH to 0"
elif [ "$HAS_FEAT" -gt 0 ]; then
  # Feature: increment minor, reset patch
  NEW_MAJOR=$MAJOR
  NEW_MINOR=$((MINOR + 1))
  NEW_PATCH=0
  VERSION_REASON="Feature(s) detected ($HAS_FEAT feature(s))"
  echo "🟢 $VERSION_REASON"
  echo "   Incrementing MINOR version: ${MINOR} → ${NEW_MINOR}"
  echo "   Resetting PATCH to 0"
else
  # Patch: increment patch
  NEW_MAJOR=$MAJOR
  NEW_MINOR=$MINOR
  NEW_PATCH=$((PATCH + 1))
  VERSION_REASON="Patch increment (only fixes/docs/chore commits)"
  echo "🔵 $VERSION_REASON"
  echo "   Incrementing PATCH version: ${PATCH} → ${NEW_PATCH}"
fi

BASE_VERSION="${NEW_MAJOR}.${NEW_MINOR}.${NEW_PATCH}"
echo "   Base version: ${MAJOR}.${MINOR}.${PATCH} → $BASE_VERSION"
echo "::endgroup::"

echo "::group::🏷️  Determining version suffix"
# Determine the scenario and apply appropriate suffix
SHORT_SHA=$(git rev-parse --short HEAD)

echo "  Is pull request: $IS_PULL_REQUEST"
if [ "$IS_PULL_REQUEST" = "true" ]; then
  echo "  PR number:       $PR_NUMBER"
fi
echo "  Current branch:  $CURRENT_BRANCH"
echo "  Default branch:  $DEFAULT_BRANCH"
echo "  Is release:      $IS_RELEASE"
echo "  Short SHA:       $SHORT_SHA"

SUFFIX_REASON=""
# Scenario 1: Pull Request (check first, as PRs are typically on non-default branches)
if [ "$IS_PULL_REQUEST" = "true" ]; then
  NEW_VERSION="${BASE_VERSION}-pr${PR_NUMBER}+${SHORT_SHA}"
  SUFFIX_REASON="Pull request build (PR #$PR_NUMBER)"
  echo "  ✅ Scenario: Pull request"

# Scenario 2: Non-default branch (ignore release input)
elif [ "$CURRENT_BRANCH" != "$DEFAULT_BRANCH" ]; then
  NEW_VERSION="${BASE_VERSION}-br+${SHORT_SHA}"
  SUFFIX_REASON="Non-default branch build ($CURRENT_BRANCH)"
  echo "  ✅ Scenario: Non-default branch"

# Scenario 3: Release (release=true and on default branch)
elif [ "$IS_RELEASE" = "true" ]; then
  NEW_VERSION="$BASE_VERSION"
  SUFFIX_REASON="Release build on default branch ($DEFAULT_BRANCH)"
  echo "  ✅ Scenario: Release build"

# Scenario 4: Main build (all other cases: push to default branch, workflow_call/dispatch with release=false)
else
  NEW_VERSION="${BASE_VERSION}-rc${COMMIT_COUNT}+${SHORT_SHA}"
  SUFFIX_REASON="Main build (default branch, non-release)"
  echo "  ✅ Scenario: Main build (RC)"
fi
echo "::endgroup::"

echo "commit_count=$COMMIT_COUNT" >> $GITHUB_OUTPUT
echo "version=$NEW_VERSION" >> $GITHUB_OUTPUT

echo ""
echo "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"
echo "📌 VERSION CALCULATION SUMMARY"
echo "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"
echo "  Last tag:        $LAST_TAG"
echo "  Commits since:   $COMMIT_COUNT"
echo "  Base version:    ${MAJOR}.${MINOR}.${PATCH} → $BASE_VERSION"
echo "  Final version:   $NEW_VERSION"
echo ""
echo "  💡 Version increment: $VERSION_REASON"
echo "  💡 Suffix applied:    $SUFFIX_REASON"
echo "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"

# Write to GitHub step summary
{
  echo "## 📌 VERSION CALCULATION SUMMARY"
  echo ""
  echo "| Field | Value |"
  echo "|-------|-------|"
  echo "| Last tag | \`$LAST_TAG\` |"
  echo "| Commits since | $COMMIT_COUNT |"
  echo "| Base version | \`${MAJOR}.${MINOR}.${PATCH}\` → \`$BASE_VERSION\` |"
  echo "| Final version | \`$NEW_VERSION\` |"
  echo ""
  echo "💡 **Version increment:** $VERSION_REASON"
  echo "💡 **Suffix applied:** $SUFFIX_REASON"
} >> "$GITHUB_STEP_SUMMARY"
