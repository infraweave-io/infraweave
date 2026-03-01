#!/bin/bash
set -euo pipefail

# Calculate version and count commits
# This script determines the semantic version based on:
# - Last git tag
# - Commits since last tag (analyzing for breaking changes, features, patches)
# - Context (PR, branch, release status)

echo "::group::🔍 Finding last tag"
# Get the latest tag, or use v0.0.0 if no tags exist
# With fetch-depth: 0 and fetch-tags: true, all tags and history are available
LAST_TAG=$(git describe --tags --abbrev=0 2>/dev/null || echo "v0.0.0")
if [ "$LAST_TAG" = "v0.0.0" ]; then
  echo "⚠️  No tags found in repository, using v0.0.0 as baseline"
else
  echo "✅ Found last tag: $LAST_TAG"
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
# Check for breaking changes and features
# Conventional commits: "BREAKING CHANGE:" or "!" in type/scope (e.g., "feat!: ..." or "feat(scope)!: ...")
# Match patterns like: "feat!:", "feat(scope)!:", or "BREAKING CHANGE:" anywhere in the message
# Use a temporary file to avoid issues with large COMMITS variable and ensure compatibility with older bash
TEMP_COMMITS=$(mktemp)
trap "rm -f '$TEMP_COMMITS'" EXIT

HAS_BREAKING=0
HAS_FEAT=0

if [ "$COMMIT_COUNT" -gt 0 ]; then
  echo "Checking commits for breaking changes and features..."
  echo ""
  
  # Get commit data with null byte separator between commits
  # Format: <hash>\0<subject>\0<full_message>\0
  # Using separate fields to avoid issues with | in commit messages
  if [ "$LAST_TAG" = "v0.0.0" ]; then
    git log --no-merges --pretty=format:"%H%x00%s%x00%B%x00" > "$TEMP_COMMITS"
  else
    git log ${LAST_TAG}..HEAD --no-merges --pretty=format:"%H%x00%s%x00%B%x00" > "$TEMP_COMMITS"
  fi
  
  # Process commits, splitting on null bytes
  # Read hash, subject, and message separately
  while IFS= read -r -d '' COMMIT_HASH && \
        IFS= read -r -d '' COMMIT_SUBJECT && \
        IFS= read -r -d '' COMMIT_MSG; do
    if [ -z "$COMMIT_HASH" ]; then
      continue
    fi
    
    # Strip newlines and get first line of subject
    COMMIT_HASH=$(echo -n "$COMMIT_HASH" | tr -d '\n\r')
    COMMIT_SUBJECT=$(echo "$COMMIT_SUBJECT" | head -n1 | tr -d '\n\r')
    # Use hash as fallback if subject is empty
    if [ -z "$COMMIT_SUBJECT" ]; then
      COMMIT_SUBJECT="(no subject)"
    fi
    
    SHORT_HASH=$(echo -n "$COMMIT_HASH" | cut -c1-7)
    
    # Format full commit message (subject + body)
    # Trim leading/trailing whitespace from body
    BODY=$(echo "$COMMIT_MSG" | sed 's/^[[:space:]]*//;s/[[:space:]]*$//')
    
    # Remove subject from body if it appears at the start (to avoid duplication)
    if [ -n "$BODY" ] && [ -n "$COMMIT_SUBJECT" ]; then
      # Check if body starts with subject (case-insensitive, allowing for whitespace)
      BODY_FIRST_LINE=$(echo "$BODY" | head -n1 | sed 's/^[[:space:]]*//')
      SUBJECT_CLEAN=$(echo "$COMMIT_SUBJECT" | sed 's/^[[:space:]]*//')
      if [ "$BODY_FIRST_LINE" = "$SUBJECT_CLEAN" ]; then
        # Remove first line from body
        BODY=$(echo "$BODY" | sed '1d' | sed 's/^[[:space:]]*//;s/[[:space:]]*$//')
      fi
    fi
    
    # Build full message: subject + body (if present)
    FULL_MSG="$COMMIT_SUBJECT"
    if [ -n "$BODY" ]; then
      FULL_MSG="$COMMIT_SUBJECT"$'\n'"$BODY"
    fi
    
    # Check for breaking changes (COMMIT_MSG can be multi-line)
    IS_BREAKING=0
    if echo "$COMMIT_MSG" | grep -qiE "(BREAKING CHANGE:|^[a-z]+(\([^)]+\))?!:)" 2>/dev/null; then
      IS_BREAKING=1
      HAS_BREAKING=1
      printf "🔴 %s - Breaking change\n" "$SHORT_HASH"
      echo ""
      echo "$FULL_MSG" | sed 's/^/    /'
      echo ""
    fi
    
    # Check for features (only if not already marked as breaking)
    if [ "$IS_BREAKING" -eq 0 ]; then
      if echo "$COMMIT_MSG" | grep -qiE "^feat" 2>/dev/null; then
        HAS_FEAT=1
        printf "🟢 %s - Feature\n" "$SHORT_HASH"
        echo ""
        echo "$FULL_MSG" | sed 's/^/    /'
        echo ""
      fi
    fi
  done < "$TEMP_COMMITS"
  
  echo ""
fi

# Clean up temp file
rm -f "$TEMP_COMMITS"
trap - EXIT

if [ "$HAS_BREAKING" -eq 0 ] && [ "$HAS_FEAT" -eq 0 ]; then
  echo "No breaking changes or features detected"
fi

echo "Breaking changes: $([ "$HAS_BREAKING" -eq 1 ] && echo "Yes" || echo "No")"
echo "Features:         $([ "$HAS_FEAT" -eq 1 ] && echo "Yes" || echo "No")"
echo "::endgroup::"

echo "::group::📈 Calculating version increment"
# Check VERSION_STABLE environment variable (default to "false")
VERSION_STABLE="${VERSION_STABLE:-false}"
echo "  VERSION_STABLE: $VERSION_STABLE"

# Calculate new version based on rules
VERSION_REASON=""
if [ "$HAS_BREAKING" -eq 1 ]; then
  if [ "$VERSION_STABLE" = "false" ]; then
    # Breaking change but VERSION_STABLE=false: treat as minor increment
    NEW_MAJOR=$MAJOR
    NEW_MINOR=$((MINOR + 1))
    NEW_PATCH=0
    VERSION_REASON="Breaking change detected (unstable update)"
    echo "🟡 $VERSION_REASON"
    echo "   Breaking change detected, but VERSION_STABLE=false"
    echo "   Incrementing MINOR version: ${MINOR} → ${NEW_MINOR}"
    echo "   Resetting PATCH to 0"
  else
    # Breaking change: increment major, reset minor and patch
    NEW_MAJOR=$((MAJOR + 1))
    NEW_MINOR=0
    NEW_PATCH=0
    VERSION_REASON="Breaking change detected"
    echo "🔴 $VERSION_REASON"
    echo "   Incrementing MAJOR version: ${MAJOR} → ${NEW_MAJOR}"
    echo "   Resetting MINOR and PATCH to 0"
  fi
elif [ "$HAS_FEAT" -eq 1 ]; then
  # Feature: increment minor, reset patch
  NEW_MAJOR=$MAJOR
  NEW_MINOR=$((MINOR + 1))
  NEW_PATCH=0
  VERSION_REASON="Feature(s) detected"
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
echo "  Release branch:  $RELEASE_BRANCH"
echo "  Is release:      $IS_RELEASE"
echo "  Is pre-release:  $IS_PRE_RELEASE"
echo "  Short SHA:       $SHORT_SHA"

SUFFIX_REASON=""
# Scenario 1: Pull Request (check first, as PRs are typically on non-default branches)
if [ "$IS_PULL_REQUEST" = "true" ]; then
  NEW_VERSION="${BASE_VERSION}-dev0+pr${PR_NUMBER}.${SHORT_SHA}"
  SUFFIX_REASON="Pull request build (PR #$PR_NUMBER)"
  echo "  ✅ Scenario: Pull request"

# Scenario 2: Non-release branch (ignore release input)
elif [ "$CURRENT_BRANCH" != "$RELEASE_BRANCH" ]; then
  NEW_VERSION="${BASE_VERSION}-dev0+br.${SHORT_SHA}"
  SUFFIX_REASON="Non-release branch build ($CURRENT_BRANCH)"
  echo "  ✅ Scenario: Non-release branch"

# Scenario 3: Release (release branch, is_release=true, is_pre_release=false)
elif [ "$IS_RELEASE" = "true" ] && [ "$IS_PRE_RELEASE" != "true" ]; then
  NEW_VERSION="$BASE_VERSION"
  SUFFIX_REASON="Release build on release branch ($RELEASE_BRANCH)"
  echo "  ✅ Scenario: Release build"

# Scenario 4: Pre-release (release branch, is_release=true, is_pre_release=true)
elif [ "$IS_RELEASE" = "true" ] && [ "$IS_PRE_RELEASE" = "true" ]; then
  NEW_VERSION="${BASE_VERSION}-rc${COMMIT_COUNT}"
  SUFFIX_REASON="On release branch and pre-release"
  echo "  ✅ Scenario: Pre-release"

# Scenario 5: Dev build (on release branch, not a release)
else
  NEW_VERSION="${BASE_VERSION}-dev${COMMIT_COUNT}+${SHORT_SHA}"
  SUFFIX_REASON="Dev build on release branch"
  echo "  ✅ Scenario: Dev build"
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
