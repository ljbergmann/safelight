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
# The bump is made from the last commit's project.yml and committed alone,
# through a separate index, so whatever else is uncommitted in this checkout
# (another session's work, say) stays out of the release and untouched.
build=$(git show HEAD:project.yml | sed -n 's/^ *CURRENT_PROJECT_VERSION: "\([0-9]*\)"/\1/p')
version=$(git show HEAD:project.yml | sed -n 's/^ *MARKETING_VERSION: "\(.*\)"/\1/p')
next=$((build + 1))
bump="s/CURRENT_PROJECT_VERSION: \"$build\"/CURRENT_PROJECT_VERSION: \"$next\"/"
blob=$(git show HEAD:project.yml | sed "$bump" | git hash-object -w --stdin)
index=$(mktemp)
GIT_INDEX_FILE=$index git read-tree HEAD
GIT_INDEX_FILE=$index git update-index --cacheinfo "100644,$blob,project.yml"
tree=$(GIT_INDEX_FILE=$index git write-tree)
rm -f "$index"
commit=$(git commit-tree "$tree" -p HEAD -m "Build $next of $version for TestFlight" -m "Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>")
git update-ref -m "testflight: build $next" HEAD "$commit"
# This checkout's index and project.yml follow, keeping their other changes.
git update-index --cacheinfo "100644,$blob,project.yml"
sed -i '' "$bump" project.yml
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
# Select Sky's and Select Subject's Neural Engine models, made by
# scripts/convert-sky-ane.py and scripts/convert-subject-coreml.py: too large for the
# repository, and a release without them would select on the CPU only.
for model in Sky-ANE:convert-sky-ane.py Subject-CoreML:convert-subject-coreml.py; do
    [ -d "$root/build/${model%%:*}.mlpackage" ] || {
        echo "build/${model%%:*}.mlpackage is missing: run scripts/${model#*:} first" >&2
        exit 1
    }
    cp -R "$root/build/${model%%:*}.mlpackage" build/
done

scripts/build-core.sh > build/core.log 2>&1 || { tail -20 build/core.log; exit 1; }
xcodegen generate -q
xcodebuild archive -project Safelight.xcodeproj -scheme Safelight -configuration Release \
    -destination 'generic/platform=iOS' -archivePath build/archive/Safelight.xcarchive \
    -derivedDataPath build/DerivedData -allowProvisioningUpdates DEVELOPMENT_TEAM="$team" -quiet
# The upload signs and signs in with the Apple account in Xcode's settings (the
# API key can't: cloud signing needs an Admin key). When that sign-in has
# expired, the export fails with "Failed to Use Accounts". A failed upload stops
# here: nothing would arrive to wait for.
xcodebuild -exportArchive -archivePath build/archive/Safelight.xcarchive \
    -exportOptionsPlist "$root/scripts/UploadOptions.plist" -exportPath build/archive/upload \
    -allowProvisioningUpdates > build/export.log 2>&1 || {
    grep -E "error:|EXPORT" build/export.log >&2
    echo "testflight: the upload of build $next failed" >&2
    exit 1
}
grep -E "Uploaded|EXPORT" build/export.log || true

app=build/archive/Safelight.xcarchive/Products/Applications/Safelight.app
if xcrun devicectl list devices 2>/dev/null | grep "$ipad" | grep -q connected; then
    xcrun devicectl device install app --device "$ipad" "$app" > /dev/null && echo "Installed on the iPad"
else
    echo "The iPad isn't connected: install it from TestFlight"
fi

cd "$root"
git worktree remove --force "$work"
scripts/testflight-groups.swift "$next" "$@"
