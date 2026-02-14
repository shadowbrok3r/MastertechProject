#!/usr/bin/env bash
set -euo pipefail

# MOBILE_DIR = MastertechMobile crate dir (where scripts/ lives)
MOBILE_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
# ROOT = workspace root (where target/ lives)
ROOT="$(cd "$MOBILE_DIR/.." && pwd)"
SDK_BUNDLE="${SDK_BUNDLE:-$HOME/.swiftpm/swift-sdks/darwin.artifactbundle}"

usage() {
  cat <<'EOF'
Usage:
  scripts/ios-build.sh sim [--release]
  scripts/ios-build.sh device [--release]

Modes:
  sim      Build simulator bundle (Linux only)
  device   Build arm64 bundle and install via xtool

Environment overrides:
  SDK_BUNDLE=...    Path to darwin.artifactbundle
  DX_BIN=dx         dx executable
  XT_BIN=xtool      xtool executable
EOF
}

[[ $# -ge 1 ]] || { usage; exit 2; }

MODE="$1"
shift || true

RELEASE_FLAG=""
for arg in "$@"; do
  case "$arg" in
    --release) RELEASE_FLAG="--release" ;;
    *) usage; exit 2 ;;
  esac
done

DX_BIN="${DX_BIN:-dx}"
XT_BIN="${XT_BIN:-xtool}"

# Use project-local shims only
export PATH="$MOBILE_DIR/scripts/shims:$PATH"

export SDKROOT=""

ensure_target() {
  local t="$1"
  rustup target add "$t" >/dev/null 2>&1 || true
}

write_cargo_config() {
  mkdir -p "$MOBILE_DIR/.cargo"
  cat > "$MOBILE_DIR/.cargo/config.toml" <<EOF
[target.x86_64-apple-ios]
linker = "/usr/lib/swift/bin/clang"
rustflags = [
  "-C", "link-arg=-fuse-ld=lld",
  "-C", "link-arg=--ld-path=$SDK_BUNDLE/toolset/bin/ld64.lld",
]

[target.aarch64-apple-ios]
linker = "/usr/lib/swift/bin/clang"
rustflags = [
  "-C", "link-arg=-fuse-ld=lld",
  "-C", "link-arg=--ld-path=$SDK_BUNDLE/toolset/bin/ld64.lld",
]
EOF
}

write_cargo_config

case "$MODE" in
  sim)
    ensure_target x86_64-apple-ios
    export SDKROOT="$SDK_BUNDLE/Developer/Platforms/iPhoneSimulator.platform/Developer/SDKs/iPhoneSimulator.sdk"

    "$DX_BIN" serve --ios \
      --target x86_64-apple-ios \
      --codesign false \
      $RELEASE_FLAG \
      --verbose
    ;;

  device)
    ensure_target aarch64-apple-ios
    export SDKROOT="$SDK_BUNDLE/Developer/Platforms/iPhoneOS.platform/Developer/SDKs/iPhoneOS.sdk"

    "$DX_BIN" build --ios \
      --target aarch64-apple-ios \
      --codesign false \
      $RELEASE_FLAG \
      --verbose

    if [[ -n "$RELEASE_FLAG" ]]; then
      APP="$ROOT/target/dx/mastertech-mobile/release/ios/MastertechMobile.app"
    else
      APP="$ROOT/target/dx/mastertech-mobile/debug/ios/MastertechMobile.app"
    fi

    if [[ ! -d "$APP" ]]; then
      echo "Expected app bundle not found at: $APP" >&2
      exit 1
    fi

    BIN="$APP/mastertech-mobile"
    if [[ -f "$BIN" ]]; then
      echo "[arch] $(file "$BIN")"
    fi

    "$XT_BIN" install "$APP"
    ;;

  *)
    usage
    exit 2
    ;;
esac
