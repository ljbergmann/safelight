#!/bin/sh
# Builds build/onnxruntime.xcframework: Microsoft's ONNX Runtime for iOS as a
# dynamic framework the app embeds.
#
# RAWmakase opens ONNX Runtime at run time (`ort`'s load-dynamic), by path,
# as the desktop does with the library it ships beside the executable.
# Microsoft's iOS build (the onnxruntime-c CocoaPod) is a static archive, so
# this links it into a dylib with the same exports. 1.23.0 is the iOS release
# nearest the desktop's 1.23.2; both are C API 23.
set -eu

VERSION=1.23.0
SHA256=756a78e0168f29840bc614b43aeb03e63673f44022e0221d21698a2c8ed747ef
URL="https://download.onnxruntime.ai/pod-archive-onnxruntime-c-$VERSION.zip"

root=$(cd "$(dirname "$0")/.." && pwd)
work="$root/build/ort-$VERSION"
out="$root/build/onnxruntime.xcframework"

if [ -f "$out/.version" ] && [ "$(cat "$out/.version")" = "$VERSION" ]; then
    exit 0
fi

mkdir -p "$work"
zip="$work/pod-archive.zip"
if [ ! -f "$zip" ] || ! echo "$SHA256  $zip" | shasum -a 256 -c - >/dev/null 2>&1; then
    curl -fsSL -o "$zip" "$URL"
    echo "$SHA256  $zip" | shasum -a 256 -c -
fi
rm -rf "$work/archive"
unzip -q "$zip" -d "$work/archive"
static="$work/archive/onnxruntime.xcframework"

# One dylib framework per slice: device, then simulator.
link() { # slice sdk target...
    slice=$1 sdk=$2
    shift 2
    fw="$work/$slice/onnxruntime.framework"
    rm -rf "$fw"
    mkdir -p "$fw"
    archs=""
    for target in "$@"; do
        archs="$archs -target $target"
    done
    # shellcheck disable=SC2086
    xcrun --sdk "$sdk" clang -dynamiclib $archs \
        -isysroot "$(xcrun --sdk "$sdk" --show-sdk-path)" \
        -Wl,-all_load "$static/$slice/onnxruntime.framework/onnxruntime" \
        -framework Foundation -framework CoreML -lc++ \
        -install_name @rpath/onnxruntime.framework/onnxruntime \
        -o "$fw/onnxruntime"
    cat >"$fw/Info.plist" <<EOF
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
    <key>CFBundleExecutable</key><string>onnxruntime</string>
    <key>CFBundleIdentifier</key><string>com.microsoft.onnxruntime</string>
    <key>CFBundleName</key><string>onnxruntime</string>
    <key>CFBundlePackageType</key><string>FMWK</string>
    <key>CFBundleShortVersionString</key><string>$VERSION</string>
    <key>CFBundleVersion</key><string>$VERSION</string>
    <key>MinimumOSVersion</key><string>18.0</string>
</dict>
</plist>
EOF
}
link ios-arm64 iphoneos arm64-apple-ios18.0
link ios-arm64_x86_64-simulator iphonesimulator \
    arm64-apple-ios18.0-simulator x86_64-apple-ios18.0-simulator

rm -rf "$out"
xcodebuild -create-xcframework \
    -framework "$work/ios-arm64/onnxruntime.framework" \
    -framework "$work/ios-arm64_x86_64-simulator/onnxruntime.framework" \
    -output "$out" >/dev/null
cp "$work/archive/LICENSE" "$out/LICENSE"
echo "$VERSION" >"$out/.version"
echo "Built $out"
