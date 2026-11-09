#!/bin/sh
# Releases the next build of Safelight to TestFlight and the iPad, so every
# tester and the iPad run the same build:
#
#   1. raises CURRENT_PROJECT_VERSION in project.yml and commits it (the
#      version, MARKETING_VERSION, stays as it is);
#   2. builds that commit from a clean worktree, so edits not yet committed
#      in this checkout never reach testers;
#   3. archives, uploads to App Store Connect and installs the same app on
#      the iPad (IPAD_UDID; skipped when unset or not connected);
#   4. puts the build in every TestFlight group and submits it for Beta App
#      Review (scripts/testflight-groups.swift, which needs the API key).
#
#     scripts/testflight.sh ["What to Test" text]
#
# Pushing the commit is left to you.
set -eu

root=$(cd "$(dirname "$0")/.." && pwd)
team=${DEVELOPMENT_TEAM:-26649VR7BH}
ipad=${IPAD_UDID:-00008103-000A593E3E6B001E}
work=${TMPDIR:-/tmp}/safelight-testflight

cd "$root"
build=$(sed -n 's/^ *CURRENT_PROJECT_VERSION: "\([0-9]*\)"/\1/p' project.yml)
next=$((build + 1))
sed -i '' "s/CURRENT_PROJECT_VERSION: \"$build\"/CURRENT_PROJECT_VERSION: \"$next\"/" project.yml
version=$(sed -n 's/^ *MARKETING_VERSION: "\(.*\)"/\1/p' project.yml)
git add project.yml
git commit -q -m "Build $next of $version for TestFlight"
commit=$(git rev-parse HEAD)
echo "Build $next of $version from $(git rev-parse --short HEAD)"

# A clean copy of the commit, its submodule from this checkout's.
rm -rf "$work"
git worktree prune
git worktree add -q --detach "$work" "$commit"
cd "$work"
git -c protocol.file.allow=always submodule update -q --init \
    --reference "$root/vendor/rawmakase" vendor/rawmakase
mkdir -p build
cp -R "$root/build/onnxruntime.xcframework" build/

scripts/build-core.sh > build/core.log 2>&1 || { tail -20 build/core.log; exit 1; }
xcodegen generate -q
xcodebuild archive -project Safelight.xcodeproj -scheme Safelight -configuration Release \
    -destination 'generic/platform=iOS' -archivePath build/archive/Safelight.xcarchive \
    -derivedDataPath build/DerivedData -allowProvisioningUpdates DEVELOPMENT_TEAM="$team" -quiet
xcodebuild -exportArchive -archivePath build/archive/Safelight.xcarchive \
    -exportOptionsPlist "$root/scripts/UploadOptions.plist" -exportPath build/archive/upload \
    -allowProvisioningUpdates 2>&1 | grep -E "error:|Uploaded|EXPORT" || true

app=build/archive/Safelight.xcarchive/Products/Applications/Safelight.app
if xcrun devicectl list devices 2>/dev/null | grep "$ipad" | grep -q connected; then
    xcrun devicectl device install app --device "$ipad" "$app" > /dev/null && echo "Installed on the iPad"
else
    echo "The iPad isn't connected: install it from TestFlight"
fi

cd "$root"
git worktree remove --force "$work"
scripts/testflight-groups.swift "$next" "$@"
