#!/bin/bash
set -e

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
cd "$SCRIPT_DIR"
PLATFORM_DIR="$SCRIPT_DIR/../desktop-demo-platform"
# shellcheck disable=SC1091
. "$SCRIPT_DIR/../../scripts/dev_build_common.sh"

is_truthy_env() {
    local value="${1:-}"
    value="$(printf '%s' "$value" | tr '[:upper:]' '[:lower:]')"
    case "$value" in
        1|true|yes|on)
            return 0
            ;;
        *)
            return 1
            ;;
    esac
}

BUILD_MODE=""

if is_truthy_env "${CI:-}" || is_truthy_env "${GITHUB_ACTIONS:-}"; then
    BUILD_MODE="release"
else
    BUILD_MODE="fast"
fi

for arg in "$@"; do
    case "$arg" in
        --fast)
            BUILD_MODE="fast"
            ;;
        --release|--optimized)
            BUILD_MODE="release"
            ;;
        *)
            echo "Unknown argument: $arg"
            echo "Usage: $0 [--fast|--release]"
            exit 1
            ;;
    esac
done

echo "Building Cranpose Demo for Web..."
echo ""

# Check if wasm-pack is installed (check common locations)
WASM_PACK="$(find_local_tool wasm-pack || true)"
if [ -z "$WASM_PACK" ]; then
    echo "Error: wasm-pack is not installed or not in PATH"
    echo "Install it with: cargo install wasm-pack"
    echo "Or add ~/.cargo/bin to your PATH"
    exit 1
fi

echo "Using wasm-pack at: $WASM_PACK"

# Avoid version check noise in CI/profile runs.
export WASM_PACK_SKIP_UPDATE_CHECK=1
export WASM_PACK_DISABLE_UPDATE_CHECK=1
WASM_PACK_LOG_LEVEL="${WASM_PACK_LOG_LEVEL:-error}"

enable_local_tmpdir
enable_local_cargo_job_limit

if [ "$BUILD_MODE" = "fast" ]; then
    enable_local_sccache
    if [ -n "${TMPDIR:-}" ]; then
        echo "Using local tmpdir: $TMPDIR"
    fi
    if [ -n "${RUSTC_WRAPPER:-}" ]; then
        echo "Using local rustc wrapper: $RUSTC_WRAPPER"
    fi
    if [ -n "${CARGO_BUILD_JOBS:-}" ]; then
        echo "Using local cargo build jobs: $CARGO_BUILD_JOBS"
    fi
    echo "Building WASM module (fast dev build, no wasm-opt)..."
    (
        cd "$PLATFORM_DIR"
        "$WASM_PACK" --log-level "$WASM_PACK_LOG_LEVEL" build --dev --target web --out-dir "$SCRIPT_DIR/pkg" --features web,renderer-wgpu --no-default-features
    )
else
    # Check if wasm-opt is available (from binaryen) for size optimization
    if command -v wasm-opt &> /dev/null; then
        echo "wasm-opt found - binary size optimization enabled"
    else
        echo "Warning: wasm-opt not found. Install binaryen for smaller WASM binaries:"
        echo "  Ubuntu/Debian: sudo apt install binaryen"
        echo "  macOS: brew install binaryen"
        echo "  Arch: pacman -S binaryen"
        echo ""
    fi

    # Build the WASM module with the workspace wasm-release profile. The native
    # release profile uses heavier LTO/codegen settings than local and CI web
    # builds can rely on consistently.
    if [ -n "${TMPDIR:-}" ]; then
        echo "Using local tmpdir: $TMPDIR"
    fi
    if [ -n "${CARGO_BUILD_JOBS:-}" ]; then
        echo "Using local cargo build jobs: $CARGO_BUILD_JOBS"
    fi
    echo "Building WASM module (optimized for size)..."

    # Run wasm-pack build, don't exit on error so we can handle it
    set +e
    (
        cd "$PLATFORM_DIR"
        "$WASM_PACK" --log-level "$WASM_PACK_LOG_LEVEL" build --profile wasm-release --target web --out-dir "$SCRIPT_DIR/pkg" --features web,renderer-wgpu --no-default-features
    )
    BUILD_RESULT=$?
    set -e

    if [ $BUILD_RESULT -ne 0 ]; then
        echo ""
        echo "wasm-pack build failed with exit code $BUILD_RESULT"
        echo "WASM size optimization is required for web builds."
        echo "Install binaryen (wasm-opt) and rerun this script."
        echo ""

        if [ "${ALLOW_UNOPTIMIZED_WASM:-0}" = "1" ]; then
            echo "ALLOW_UNOPTIMIZED_WASM=1 set - retrying with --dev (unoptimized)."
            (
                cd "$PLATFORM_DIR"
                "$WASM_PACK" --log-level "$WASM_PACK_LOG_LEVEL" build --dev --target web --out-dir "$SCRIPT_DIR/pkg" --features web,renderer-wgpu --no-default-features
            )
            BUILD_RESULT=$?
            if [ $BUILD_RESULT -ne 0 ]; then
                echo "Build failed even with --dev"
                exit 1
            fi
        else
            exit 1
        fi
    fi
fi

# Show resulting binary size
if [ -f "pkg/desktop_app_bg.wasm" ]; then
    SIZE=$(du -h pkg/desktop_app_bg.wasm | cut -f1)
    SIZE_BYTES=$(wc -c < pkg/desktop_app_bg.wasm | tr -d '[:space:]')
    echo ""
    echo "WASM binary size: $SIZE"
    if [ "$BUILD_MODE" = "release" ]; then
        MAX_WASM_BYTES="${CRANPOSE_WEB_RELEASE_MAX_WASM_BYTES:-14680064}"
        case "$MAX_WASM_BYTES" in
            ''|*[!0-9]*)
                echo "Invalid CRANPOSE_WEB_RELEASE_MAX_WASM_BYTES: $MAX_WASM_BYTES"
                exit 1
                ;;
        esac
        echo "WASM release size budget: $SIZE_BYTES / $MAX_WASM_BYTES bytes"
        if [ "$SIZE_BYTES" -gt "$MAX_WASM_BYTES" ]; then
            echo "WASM release size budget failed: $SIZE_BYTES bytes exceeds $MAX_WASM_BYTES bytes"
            exit 1
        fi
    fi
fi

echo ""
echo "Build complete! 🎉"
echo ""
if [ "$BUILD_MODE" = "fast" ]; then
    echo "Mode: fast local verification build"
    echo "For optimized output, rerun with: ./build-web.sh --release"
else
    echo "Mode: optimized release build"
fi
echo ""
echo "To run the demo:"
echo "1. Start a local web server in this directory:"
echo "   python3 -m http.server 8080"
echo "   or"
echo "   npx serve ."
echo ""
echo "2. Open http://localhost:8080 in your browser"
echo ""
echo "Note: The browser demo defaults to the stable WebGL2 path."
echo "      Use ?backend=webgpu to opt into the browser WebGPU path."
