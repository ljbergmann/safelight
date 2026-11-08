#!/bin/sh
# Build rawmakase-core for the iPad and the iPad simulator, generate its Swift
# bindings, and package both libraries as build/RawmakaseCore.xcframework,
# which the Xcode project links.
#
# Usage: scripts/build-core.sh [debug|release]   (default: release)
set -eu

profile="${1:-release}"
root="$(cd "$(dirname "$0")/.." && pwd)"
cd "$root/core"

# Homebrew's rustc can shadow rustup's on PATH and has no iOS targets: use the
# rustup toolchain's binaries explicitly.
PATH="$(dirname "$(rustup which --toolchain stable rustc)"):$PATH"
export PATH

flag=""
[ "$profile" = release ] && flag="--release"

for target in aarch64-apple-ios aarch64-apple-ios-sim; do
    cargo build $flag --lib --target "$target"
done

# The API is read from a host build of the library.
cargo build $flag --lib
out="$root/build"
rm -rf "$out/headers" "$out/RawmakaseCore.xcframework"
mkdir -p "$out/headers" "$root/app/Generated"
cargo run -q --bin uniffi-bindgen -- generate \
    --library "target/$profile/librawmakase_core.dylib" \
    --language swift --out-dir "$out/bindings"

cp "$out/bindings/rawmakase_core.swift" "$root/app/Generated/"
cp "$out/bindings/rawmakase_coreFFI.h" "$out/headers/"
# Xcode finds a framework's module map only under this name.
cp "$out/bindings/rawmakase_coreFFI.modulemap" "$out/headers/module.modulemap"

xcodebuild -create-xcframework \
    -library "target/aarch64-apple-ios/$profile/librawmakase_core.a" -headers "$out/headers" \
    -library "target/aarch64-apple-ios-sim/$profile/librawmakase_core.a" -headers "$out/headers" \
    -output "$out/RawmakaseCore.xcframework"
