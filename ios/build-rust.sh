#!/bin/sh
# Builds the Rust session for the platform Xcode is building, into the products directory.
set -eu
case "$PLATFORM_NAME" in
  iphonesimulator) target=aarch64-apple-ios-sim ;;
  iphoneos) target=aarch64-apple-ios ;;
  *) echo "Unsupported platform: $PLATFORM_NAME" >&2; exit 1 ;;
esac
if [ "$CONFIGURATION" = "Release" ]; then profile=release; flag=--release; else profile=debug; flag=; fi
cd "$SRCROOT/.."
# A clean environment: Xcode's SDK settings break Cargo's host and C dependency builds.
env -i HOME="$HOME" PATH="$HOME/.cargo/bin:/opt/homebrew/bin:/usr/bin:/bin" \
  CMAKE_POLICY_VERSION_MINIMUM=3.5 \
  IPHONEOS_DEPLOYMENT_TARGET="${IPHONEOS_DEPLOYMENT_TARGET:-17.0}" \
  cargo build -p flexspeak-ios --target "$target" $flag
mkdir -p "$BUILT_PRODUCTS_DIR"
cp "target/$target/$profile/libflexspeak_ios.a" "$BUILT_PRODUCTS_DIR/libflexspeak_ios.a"
