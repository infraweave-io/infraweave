#!/bin/bash
set -euo pipefail

# Script to build Linux Docker images locally (Linux or macOS)
# Usage: ./build_image.sh [OPTIONS] [bin] [platform]
# Example: ./build_image.sh cli linux/amd64

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
DOCKER_JSON="${SCRIPT_DIR}/.github/vars/default.docker.json"
TARGETS_JSON="${SCRIPT_DIR}/.github/vars/default.targets.json"

# Command-line flags
SKIP_BINARY=false
FORCE_REBUILD=false
LIST_ONLY=false

# Parse command-line arguments
while [[ $# -gt 0 ]]; do
    case $1 in
        --skip-binary)
            SKIP_BINARY=true
            shift
            ;;
        --force)
            FORCE_REBUILD=true
            shift
            ;;
        --list)
            LIST_ONLY=true
            shift
            ;;
        --help|-h)
            echo "Usage: $0 [OPTIONS] [bin] [platform]"
            echo ""
            echo "Options:"
            echo "  --skip-binary    Skip building the binary (use existing)"
            echo "  --force          Force rebuild even if binary exists"
            echo "  --list           List all available combinations"
            echo "  --help, -h       Show this help message"
            echo ""
            echo "Examples:"
            echo "  $0 cli linux/amd64"
            echo "  $0 --list"
            echo "  $0 --skip-binary cli linux/amd64"
            exit 0
            ;;
        *)
            break
            ;;
    esac
done

# Check if jq is installed
if ! command -v jq &> /dev/null; then
    echo "Error: jq is not installed. Please install it first." >&2
    echo "  On Ubuntu/Debian: sudo apt-get install jq" >&2
    echo "  On macOS: brew install jq" >&2
    exit 1
fi

# Check if rustup is installed
if ! command -v rustup &> /dev/null; then
    echo "Error: rustup is not installed. Please install Rust toolchain first." >&2
    echo "  Visit: https://rustup.rs/" >&2
    exit 1
fi

# Check if cargo is installed
if ! command -v cargo &> /dev/null; then
    echo "Error: cargo is not installed. Please install Rust toolchain first." >&2
    echo "  Visit: https://rustup.rs/" >&2
    exit 1
fi

# Check if cross is installed (required for building Linux targets)
if ! command -v cross &> /dev/null; then
    echo "Error: cross is not installed but is required for building Linux targets." >&2
    echo "  Install it with: cargo install cross --git https://github.com/cross-rs/cross" >&2
    exit 1
fi

# Check if docker buildx bake is available
if ! docker buildx bake --help &> /dev/null; then
    echo "Error: docker buildx bake is not available." >&2
    echo "  Please ensure Docker Buildx is installed and enabled." >&2
    exit 1
fi

# Read the Docker images configuration
if [ ! -f "$DOCKER_JSON" ]; then
    echo "Error: $DOCKER_JSON not found" >&2
    exit 1
fi

if [ ! -f "$TARGETS_JSON" ]; then
    echo "Error: $TARGETS_JSON not found" >&2
    exit 1
fi

# Validate JSON files are valid
if ! jq empty "$DOCKER_JSON" 2>/dev/null; then
    echo "Error: $DOCKER_JSON is not valid JSON" >&2
    exit 1
fi

if ! jq empty "$TARGETS_JSON" 2>/dev/null; then
    echo "Error: $TARGETS_JSON is not valid JSON" >&2
    exit 1
fi

# Parse platform to target key format (always musl)
# linux/amd64 -> linux-amd64-musl
# linux/arm64 -> linux-arm64-musl
platform_to_target_key() {
    local platform=$1
    local key=$(echo "$platform" | sed 's|linux/||' | sed 's|/|-|g')
    echo "linux-${key}-musl"
}

# Extract platform architecture from platform string
# linux/amd64 -> amd64
# linux/arm64 -> arm64
extract_platform_arch() {
    local platform=$1
    echo "$platform" | sed 's|linux/||'
}

# Get available combinations (bin|platform; target is always musl)
get_combinations() {
    jq -r '.[] | .bin as $bin | .platforms[] | "\($bin)|\(.)"' "$DOCKER_JSON"
}

# Get list of binary names
get_bins() {
    jq -r '.[].bin' "$DOCKER_JSON"
}

# Get platforms for a given binary
get_platforms_for_bin() {
    local bin=$1
    jq -r --arg bin "$bin" '.[] | select(.bin == $bin) | .platforms[]' "$DOCKER_JSON"
}

# List all available combinations
list_combinations() {
    local combinations=()
    while IFS= read -r line; do
        combinations+=("$line")
    done < <(get_combinations)

    echo "Available combinations:"
    echo ""
    local i=1
    for combo in "${combinations[@]}"; do
        IFS='|' read -r bin platform <<< "$combo"
        printf "  %2d) bin=%-20s platform=%s\n" "$i" "$bin" "$platform"
        ((i++))
    done
    echo ""
}

# Select combination interactively: first binary, then platform (only if more than one)
select_combination() {
    local bins=()
    while IFS= read -r line; do
        bins+=("$line")
    done < <(get_bins)

    echo "Select binary:" >&2
    echo "" >&2
    local i=1
    for b in "${bins[@]}"; do
        printf "  %2d) %s\n" "$i" "$b" >&2
        ((i++))
    done
    echo "" >&2
    echo -n "Select binary (1-${#bins[@]}): " >&2
    read bin_selection

    if ! [[ "$bin_selection" =~ ^[0-9]+$ ]] || [ "$bin_selection" -lt 1 ] || [ "$bin_selection" -gt "${#bins[@]}" ]; then
        echo "Error: Invalid selection" >&2
        exit 1
    fi

    local bin="${bins[$((bin_selection-1))]}"
    local platforms=()
    while IFS= read -r line; do
        platforms+=("$line")
    done < <(get_platforms_for_bin "$bin")

    local platform
    if [ "${#platforms[@]}" -gt 1 ]; then
        echo "" >&2
        echo "Select platform for $bin:" >&2
        echo "" >&2
        i=1
        for p in "${platforms[@]}"; do
            printf "  %2d) %s\n" "$i" "$p" >&2
            ((i++))
        done
        echo "" >&2
        echo -n "Select platform (1-${#platforms[@]}): " >&2
        read platform_selection

        if ! [[ "$platform_selection" =~ ^[0-9]+$ ]] || [ "$platform_selection" -lt 1 ] || [ "$platform_selection" -gt "${#platforms[@]}" ]; then
            echo "Error: Invalid selection" >&2
            exit 1
        fi
        platform="${platforms[$((platform_selection-1))]}"
    else
        platform="${platforms[0]}"
        echo "Platform: $platform" >&2
    fi

    # Output the selected combination to stdout (for capture)
    echo "${bin}|${platform}"
}

# Target is always musl for Linux Docker images
TARGET_DEFAULT=musl

# Build binary function (always uses cross for Linux musl targets)
build_binary() {
    local bin=$1
    local rust_target=$2
    local platform_arch=$3

    local binary_path="target/${rust_target}/release/${bin}"
    local binary_dest="binaries/${bin}-linux-${platform_arch}-musl"

    # Check if binary already exists and is up-to-date
    if [ "$FORCE_REBUILD" = "false" ] && [ -f "$binary_dest" ] && [ -f "$binary_path" ]; then
        # If source is not newer than destination, skip rebuild
        if [ ! "$binary_path" -nt "$binary_dest" ]; then
            echo "Binary already exists and is up-to-date: $binary_dest"
            echo "  Use --force to rebuild"
            return 0
        fi
    fi

    echo ""
    echo "Building binary..."
    echo "  bin: $bin"
    echo "  rust_target: $rust_target"
    echo "  using: cross"
    echo ""

    # Ensure Rust target is installed
    if ! rustup target list --installed | grep -q "^${rust_target}$"; then
        echo "Installing Rust target: $rust_target"
        rustup target add "$rust_target"
    fi

    # Build the binary using cross
    if ! cross build --release --locked --target "$rust_target" --bin "$bin"; then
        echo "" >&2
        echo "Error: Cross compilation failed for target: $rust_target" >&2
        echo "  Suggestion: Try running 'cargo clean' and then retry the build" >&2
        exit 1
    fi

    # Verify binary was built
    if [ ! -f "$binary_path" ]; then
        echo "Error: Binary not found at $binary_path" >&2
        exit 1
    fi

    # Create binaries directory
    mkdir -p binaries

    # Copy binary to expected location
    echo ""
    echo "Copying binary to expected location..."
    echo "  from: $binary_path"
    echo "  to: $binary_dest"
    cp "$binary_path" "$binary_dest"
    chmod +x "$binary_dest"

    echo "  Binary ready for Docker build"
    echo ""
}

# Main logic
if [ "$LIST_ONLY" = "true" ]; then
    list_combinations
    exit 0
fi

if [ $# -eq 2 ]; then
    # Arguments provided
    BIN=$1
    PLATFORM=$2
    TARGET=$TARGET_DEFAULT
elif [ $# -eq 0 ]; then
    # Interactive selection
    SELECTED=$(select_combination)
    IFS='|' read -r BIN PLATFORM <<< "$SELECTED"
    TARGET=$TARGET_DEFAULT
else
    echo "Usage: $0 [OPTIONS] [bin] [platform]" >&2
    echo "  Example: $0 cli linux/amd64" >&2
    echo "  Or run without arguments for interactive selection" >&2
    echo "  Use --help for more options" >&2
    exit 1
fi

# Validate the combination exists
VALID_COMBO=$(jq -r --arg bin "$BIN" --arg platform "$PLATFORM" \
    '[.[] | select(.bin == $bin and (.platforms | index($platform)))] | length' \
    "$DOCKER_JSON")

if [ "$VALID_COMBO" -eq 0 ]; then
    echo "Error: Invalid combination: bin=$BIN, platform=$PLATFORM" >&2
    exit 1
fi

# Get bake-file
BAKE_FILE=$(jq -r --arg bin "$BIN" --arg platform "$PLATFORM" \
    '[.[] | select(.bin == $bin and (.platforms | index($platform)))] | .[0]."bake-file"' \
    "$DOCKER_JSON")

if [ -z "$BAKE_FILE" ] || [ "$BAKE_FILE" = "null" ]; then
    echo "Error: Could not find bake-file for combination" >&2
    exit 1
fi

# Validate bake file exists
if [ ! -f "${SCRIPT_DIR}/${BAKE_FILE}" ]; then
    echo "Error: Bake file not found: ${SCRIPT_DIR}/${BAKE_FILE}" >&2
    exit 1
fi

# Convert platform to target key (always musl)
TARGET_KEY=$(platform_to_target_key "$PLATFORM")

# Get rust_target from targets.json
RUST_TARGET=$(jq -r --arg key "$TARGET_KEY" '.[$key].rust_target' "$TARGETS_JSON")

if [ -z "$RUST_TARGET" ] || [ "$RUST_TARGET" = "null" ]; then
    echo "Error: Could not find rust_target for key: $TARGET_KEY" >&2
    exit 1
fi

# Extract platform architecture once
PLATFORM_ARCH=$(extract_platform_arch "$PLATFORM")

echo ""
echo "Selected combination:"
echo "  bin: $BIN"
echo "  platform: $PLATFORM"
echo "  target: $TARGET"
echo "  bake-file: $BAKE_FILE"
echo "  rust_target: $RUST_TARGET"
echo ""

echo "Building Linux binary with cross"

cd "$SCRIPT_DIR"

# Build the binary (unless skipped)
if [ "$SKIP_BINARY" = "false" ]; then
    build_binary "$BIN" "$RUST_TARGET" "$PLATFORM_ARCH"
else
    echo "Skipping binary build (--skip-binary flag set)"
    echo ""
fi

# Set environment variables that bake might need
export REGISTRY="${REGISTRY:-localhost}"
VERSION_BASE="${VERSION:-dev}"
export VERSION="${VERSION_BASE}-${PLATFORM_ARCH}"

# Build the image
echo "Building Docker image..."
echo "  Using bake file: $BAKE_FILE"
echo "  Platform: $PLATFORM"
echo "  Image tag: ${REGISTRY}/${BIN}:${VERSION}"
echo ""

# Build with bake
if ! docker buildx bake \
    --file "$BAKE_FILE" \
    --set "*.platform=$PLATFORM" \
    --load; then
    echo "" >&2
    echo "Error: Docker buildx bake failed" >&2
    echo "" >&2
    echo "Multi-platform builds need to be enabled. See:" >&2
    echo "  https://docs.docker.com/build/building/multi-platform/" >&2
    echo "" >&2
    echo "Note: Multi-platform storage does not work with rootless Docker." >&2
    exit 1
fi

echo ""
echo "Build completed successfully!"
