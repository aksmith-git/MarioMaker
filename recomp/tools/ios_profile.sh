#!/bin/bash
# usage: tools/ios_profile.sh <bundle id> <team id> [out .mobileprovision]
# Has Xcode make (or renew) a development provisioning profile for <bundle id> that carries what the app needs on a
# device - com.apple.developer.kernel.extended-virtual-addressing (the guest memory window, runtime/rt.h) and
# com.apple.developer.kernel.increased-memory-limit (the 3+ GB the game uses) - for the team's registered devices, by
# building an empty stub app with automatic signing. Prints the profile's path (RECOMP_IOS_PROFILE for
# tools/make_ios_app.sh). Needs the team's Apple ID in Xcode (Settings > Accounts) and xcodegen (Homebrew).
set -euo pipefail
ID=$1 TEAM=$2
OUT=${3:-build/ios-profiles/$ID.mobileprovision}
command -v xcodegen >/dev/null || { echo "ios_profile: needs xcodegen (brew install xcodegen)" >&2; exit 1; }
work=$(mktemp -d)
trap 'rm -rf "$work"' EXIT
mkdir -p "$work/Stub"
echo 'int main(void) { return 0; }' > "$work/Stub/main.m"
cat > "$work/project.yml" <<EOF
name: Stub
targets:
  Stub:
    type: application
    platform: iOS
    deploymentTarget: "17.0"
    sources: [Stub]
    info:
      path: Stub/Info.plist
      properties: {UILaunchScreen: {}}
    entitlements:
      path: Stub/Stub.entitlements
      properties:
        com.apple.developer.kernel.extended-virtual-addressing: true
        com.apple.developer.kernel.increased-memory-limit: true
    settings:
      PRODUCT_BUNDLE_IDENTIFIER: $ID
      DEVELOPMENT_TEAM: $TEAM
      CODE_SIGN_STYLE: Automatic
EOF
(cd "$work" && xcodegen generate --quiet &&
    xcodebuild -project Stub.xcodeproj -target Stub -sdk iphoneos -configuration Debug -allowProvisioningUpdates \
        -allowProvisioningDeviceRegistration CONFIGURATION_BUILD_DIR="$work/out" build > "$work/xcodebuild.log" 2>&1) || {
    grep -E "error:|warning: .*(provision|sign|account)" "$work/xcodebuild.log" | sort -u | head -20 >&2
    echo "ios_profile: xcodebuild failed (is the team's Apple ID signed in to Xcode > Settings > Accounts?)" >&2
    exit 1
}
prof="$work/out/Stub.app/embedded.mobileprovision"
[ -f "$prof" ] || { echo "ios_profile: the stub app has no profile" >&2; exit 1; }
security cms -D -i "$prof" > "$work/p.plist"
for k in com.apple.developer.kernel.extended-virtual-addressing com.apple.developer.kernel.increased-memory-limit; do
    /usr/libexec/PlistBuddy -c "Print :Entitlements:$k" "$work/p.plist" >/dev/null 2>&1 ||
        echo "ios_profile: the profile lacks $k (enable it for $ID at developer.apple.com > Identifiers)" >&2
done
mkdir -p "$(dirname "$OUT")"
cp "$prof" "$OUT"
echo "$OUT"
