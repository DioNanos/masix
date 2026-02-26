#!/usr/bin/env bash

set -euo pipefail

usage() {
  cat <<'EOF'
Usage:
  scripts/build_stt_whisper_asset.sh <target-id> <out-dir>

Supported target-id:
  - linux-x86_64
  - macos-x86_64
  - macos-aarch64
  - android-aarch64-termux

Environment variables:
  WHISPER_CPP_SRC        Source checkout path (default: .cache/whisper.cpp)
  WHISPER_CPP_REPO       Upstream repository URL
  WHISPER_CPP_REF        Optional git ref/tag/commit for whisper.cpp
  ANDROID_NDK_ROOT/HOME  Required for android-aarch64-termux
  CMAKE_BUILD_PARALLEL_LEVEL  Optional parallel build jobs
EOF
}

die() {
  echo "ERROR: $*" >&2
  exit 1
}

run() {
  echo "+ $*"
  "$@"
}

if [[ "${1:-}" == "-h" || "${1:-}" == "--help" ]]; then
  usage
  exit 0
fi

TARGET_ID="${1:-}"
OUT_DIR="${2:-}"
[[ -n "$TARGET_ID" && -n "$OUT_DIR" ]] || { usage; exit 1; }

case "$TARGET_ID" in
  linux-x86_64|macos-x86_64|macos-aarch64|android-aarch64-termux) ;;
  *) die "Unsupported target-id '$TARGET_ID'" ;;
esac

ROOT_DIR="$(pwd)"
WHISPER_CPP_SRC="${WHISPER_CPP_SRC:-$ROOT_DIR/.cache/whisper.cpp}"
WHISPER_CPP_REPO="${WHISPER_CPP_REPO:-https://github.com/ggerganov/whisper.cpp.git}"
WHISPER_CPP_REF="${WHISPER_CPP_REF:-}"
BUILD_DIR="$ROOT_DIR/.build/stt-whisper/${TARGET_ID}"
ASSET_NAME="masix-stt-whisper-cli-${TARGET_ID}"
ASSET_PATH="${OUT_DIR%/}/$ASSET_NAME"

mkdir -p "$OUT_DIR"
mkdir -p "$(dirname "$WHISPER_CPP_SRC")"
mkdir -p "$BUILD_DIR"

if [[ ! -d "$WHISPER_CPP_SRC/.git" ]]; then
  run git clone --depth 1 "$WHISPER_CPP_REPO" "$WHISPER_CPP_SRC"
else
  run git -C "$WHISPER_CPP_SRC" fetch --tags --force origin
fi

if [[ -n "$WHISPER_CPP_REF" ]]; then
  if git -C "$WHISPER_CPP_SRC" rev-parse --verify --quiet "$WHISPER_CPP_REF^{commit}" >/dev/null; then
    run git -C "$WHISPER_CPP_SRC" checkout --detach "$WHISPER_CPP_REF"
  else
    run git -C "$WHISPER_CPP_SRC" fetch --depth 1 origin "$WHISPER_CPP_REF"
    run git -C "$WHISPER_CPP_SRC" checkout --detach FETCH_HEAD
  fi
else
  DEFAULT_REMOTE_REF="$(git -C "$WHISPER_CPP_SRC" symbolic-ref --quiet --short refs/remotes/origin/HEAD 2>/dev/null || true)"
  if [[ -z "$DEFAULT_REMOTE_REF" ]]; then
    DEFAULT_REMOTE_REF="origin/master"
  fi
  run git -C "$WHISPER_CPP_SRC" checkout --detach "$DEFAULT_REMOTE_REF"
fi

JOBS="${CMAKE_BUILD_PARALLEL_LEVEL:-$(getconf _NPROCESSORS_ONLN 2>/dev/null || echo 2)}"

cmake_configure_common=(
  -S "$WHISPER_CPP_SRC"
  -B "$BUILD_DIR"
  -DCMAKE_BUILD_TYPE=Release
  -DWHISPER_BUILD_TESTS=OFF
  -DWHISPER_BUILD_EXAMPLES=ON
)

case "$TARGET_ID" in
  linux-x86_64)
    run cmake "${cmake_configure_common[@]}"
    ;;
  macos-x86_64)
    run cmake "${cmake_configure_common[@]}" -DCMAKE_OSX_ARCHITECTURES=x86_64
    ;;
  macos-aarch64)
    run cmake "${cmake_configure_common[@]}" -DCMAKE_OSX_ARCHITECTURES=arm64
    ;;
  android-aarch64-termux)
    ANDROID_NDK_PATH="${ANDROID_NDK_ROOT:-${ANDROID_NDK_HOME:-}}"
    [[ -n "$ANDROID_NDK_PATH" ]] || die "ANDROID_NDK_ROOT or ANDROID_NDK_HOME is required for android-aarch64-termux"
    TOOLCHAIN_FILE="$ANDROID_NDK_PATH/build/cmake/android.toolchain.cmake"
    [[ -f "$TOOLCHAIN_FILE" ]] || die "Android NDK toolchain file not found: $TOOLCHAIN_FILE"
    run cmake "${cmake_configure_common[@]}" \
      -DCMAKE_TOOLCHAIN_FILE="$TOOLCHAIN_FILE" \
      -DANDROID_ABI=arm64-v8a \
      -DANDROID_PLATFORM=24 \
      -DANDROID_STL=c++_static
    ;;
esac

if ! cmake --build "$BUILD_DIR" --parallel "$JOBS" --target whisper-cli; then
  echo "whisper-cli target failed, retrying legacy target 'main'" >&2
  run cmake --build "$BUILD_DIR" --parallel "$JOBS" --target main
fi

built_candidates=(
  "$BUILD_DIR/bin/whisper-cli"
  "$BUILD_DIR/bin/whisper-cpp"
  "$BUILD_DIR/bin/main"
)

BUILT_BIN=""
for candidate in "${built_candidates[@]}"; do
  if [[ -f "$candidate" ]]; then
    BUILT_BIN="$candidate"
    break
  fi
done

[[ -n "$BUILT_BIN" ]] || die "Built whisper CLI binary not found in expected locations"

run cp "$BUILT_BIN" "$ASSET_PATH"
chmod +x "$ASSET_PATH"

if command -v sha256sum >/dev/null 2>&1; then
  SHA256="$(sha256sum "$ASSET_PATH" | awk '{print $1}')"
elif command -v shasum >/dev/null 2>&1; then
  SHA256="$(shasum -a 256 "$ASSET_PATH" | awk '{print $1}')"
else
  SHA256=""
fi

echo "Asset ready: $ASSET_PATH"
if [[ -n "$SHA256" ]]; then
  echo "SHA256: $SHA256"
fi
