#!/bin/bash
set -euo pipefail

# Validate that all required binaries for Docker images are configured
# This script checks that the binaries matrix includes all linux-musl targets needed by docker images

# If DOCKER_IMAGES is empty or unset, use default.docker.json
if [ -z "${DOCKER_IMAGES:-}" ]; then
  DOCKER_IMAGES=$(cat .github/vars/default.docker.json)
fi

# If TARGETS is empty or unset, use default.targets.json
if [ -z "${TARGETS:-}" ]; then
  TARGETS=$(cat .github/vars/default.targets.json)
fi

# If BINARIES is empty or unset, use default.binaries.json
if [ -z "${BINARIES:-}" ]; then
  BINARIES=$(cat .github/vars/default.binaries.json)
fi

# Initialize summary
summary="# Docker Binary Validation Results\n\n"
errors=()
warnings=()

# Function to convert platform to musl target name
# linux/amd64 -> linux-amd64-musl
# linux/arm64 -> linux-arm64-musl
platform_to_musl_target() {
  local platform=$1
  echo "$platform" | sed 's/\//-/' | sed 's/$/-musl/'
}

# Process each docker image configuration
while IFS= read -r docker_entry; do
  bin=$(echo "$docker_entry" | jq -r '.bin')
  platforms=$(echo "$docker_entry" | jq -r '.platforms[]')
  bake_file=$(echo "$docker_entry" | jq -r '."bake-file"')
  
  # Track errors for this entry
  entry_errors=()
  entry_summary=""
  
  # Check if bake file exists
  if [ ! -f "$bake_file" ]; then
    error_msg="Bake file '${bake_file}' does not exist for binary '${bin}'"
    entry_errors+=("$error_msg")
    errors+=("$error_msg")
    entry_summary="${entry_summary}❌ **ERROR**: ${error_msg}\n"
  fi
  
  # Check if bin exists in BINARIES
  bin_exists=$(echo "$BINARIES" | jq -r --arg bin "$bin" '.[] | select(.bin == $bin) | .bin')
  if [ -z "$bin_exists" ]; then
    error_msg="Docker image '${bin}' requires binary '${bin}' but it's not in BINARIES configuration"
    entry_errors+=("$error_msg")
    errors+=("$error_msg")
    entry_summary="${entry_summary}❌ **ERROR**: ${error_msg}\n"
  else
    # Get targets for this binary
    bin_targets=$(echo "$BINARIES" | jq -r --arg bin "$bin" '.[] | select(.bin == $bin) | .targets[]')
    
    # For each platform, check if the required musl target exists
    while IFS= read -r platform; do
      required_target=$(platform_to_musl_target "$platform")
      
      # Check if this target exists in the binary's targets
      target_found=$(echo "$bin_targets" | grep -Fx "$required_target" || true)
      if [ -z "$target_found" ]; then
        error_msg="Platform '${platform}' requires target '${required_target}' but it's not configured for binary '${bin}'"
        entry_errors+=("$error_msg")
        errors+=("$error_msg")
        entry_summary="${entry_summary}  ❌ **MISSING**: Platform \`${platform}\` → Required target \`${required_target}\` not configured\n"
      fi
      
      # Also verify the target exists in TARGETS
      target_exists=$(echo "$TARGETS" | jq -r --arg target "$required_target" 'has($target)')
      if [ "$target_exists" != "true" ]; then
        error_msg="Target '${required_target}' required by Docker image '${bin}' (platform: ${platform}) does not exist in TARGETS configuration"
        entry_errors+=("$error_msg")
        errors+=("$error_msg")
        entry_summary="${entry_summary}  ❌ **ERROR**: Target \`${required_target}\` definition missing in TARGETS (platform: \`${platform}\`)\n"
      fi
    done <<< "$platforms"
  fi
  
  # Add summary for this entry: detailed if errors, simple checkmark if OK
  if [ ${#entry_errors[@]} -gt 0 ]; then
    summary="${summary}## ❌ Binary: \`${bin}\`\n\n"
    summary="${summary}${entry_summary}\n"
  else
    summary="${summary}✅ \`${bin}\`\n"
  fi
done < <(echo "$DOCKER_IMAGES" | jq -c '.[]')

# Final summary
summary="${summary}---\n\n"

if [ ${#errors[@]} -gt 0 ]; then
  summary="${summary}## ❌ Validation Failed\n\n"
  summary="${summary}Found ${#errors[@]} error(s):\n\n"
  for i in "${!errors[@]}"; do
    summary="${summary}$((i+1)). ${errors[$i]}\n"
  done
  summary="${summary}\n### How to fix:\n\n"
  summary="${summary}1. Ensure all required binary targets (linux-*-musl) are listed in \`.github/vars/default.binaries.json\`\n"
  summary="${summary}2. Ensure all target definitions exist in \`.github/vars/default.targets.json\`\n"
  summary="${summary}3. Ensure all bake files exist.\n"
  
  echo "::error::Docker binary validation failed with ${#errors[@]} error(s)"
  for error in "${errors[@]}"; do
    echo "::error::$error"
  done
  
  # Write to GitHub step summary
  echo -e "$summary" >> "$GITHUB_STEP_SUMMARY"
  
  exit 1
else
  summary="${summary}## ✅ Validation Passed\n\n"
  summary="${summary}All required binaries are configured for Docker images (linux-*-musl).\n"
  
  echo "✓ All required binaries are configured for Docker images"
  
  # Write to GitHub step summary
  echo -e "$summary" >> "$GITHUB_STEP_SUMMARY"
  
  exit 0
fi
