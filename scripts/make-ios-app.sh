#!/bin/sh
# Build the Rust core + Swift app for iOS / iPadOS and assemble an .app bundle.
#   scripts/make-ios-app.sh            → build/ios-sim/SMCraft.app     (iOS Simulator, arm64, ad-hoc signed)
#   scripts/make-ios-app.sh --device   → build/ios-device/SMCraft.app  (iPhone / iPad, arm64)
#
# Same Swift package as macOS (app/Package.swift), cross-compiled with `swift build --triple`;
# the core is linked from core/target/<rust target>/release (DECISIONS.md, "iOS packaging").
#
# Device builds need the owner's signing: set MMK_IOS_SIGN_IDENTITY (a codesigning identity,
# e.g. "Apple Development: …", see `security find-identity -p codesigning`) and MMK_IOS_PROFILE
# (a .mobileprovision for the bundle id that lists the device). MMK_IOS_BUNDLE_ID overrides the
# bundle id (a free team needs its own). Without them the bundle is left unsigned.
#
# Simulator: xcrun simctl install booted build/ios-sim/SMCraft.app && xcrun simctl launch booted <id>
# (environment for the app: SIMCTL_CHILD_MMK_COURSE=… etc., see README).
set -eu
root=$(cd "$(dirname "$0")/.." && pwd)
identity="$root/app/Sources/MMKApp/Identity.swift"
field() { sed -n "s/^ *static let $1 = \"\(.*\)\".*/\1/p" "$identity"; }
name=$(field name)
bundle_id=${MMK_IOS_BUNDLE_ID:-$(field bundleID)}
version=$(field version)
[ -n "$name" ] && [ -n "$bundle_id" ] && [ -n "$version" ] || { echo "cannot read $identity" >&2; exit 1; }
build_number=$(git -C "$root" rev-list --count HEAD 2>/dev/null || echo 1)

if [ "${1:-}" = "--device" ]; then
    rust_target=aarch64-apple-ios; sdk=iphoneos; triple=arm64-apple-ios17.0; platform=iPhoneOS; out="$root/build/ios-device"
else
    rust_target=aarch64-apple-ios-sim; sdk=iphonesimulator; triple=arm64-apple-ios17.0-simulator; platform=iPhoneSimulator; out="$root/build/ios-sim"
fi
sdk_path=$(xcrun --sdk "$sdk" --show-sdk-path)
lib_dir="$root/core/target/$rust_target/release"

echo "== Rust core ($rust_target)"
rustup target list --installed | grep -qx "$rust_target" || rustup target add "$rust_target"
(cd "$root/core" && cargo build --release -p mmk-ffi --target "$rust_target")

echo "== Swift app ($triple)"
# own scratch folder: SwiftPM points .build/release at the last platform built, which would
# confuse the macOS scripts
swift_build() {
    (cd "$root/app" && swift build -c release --scratch-path "$root/app/.build/ios-$sdk" \
        --triple "$triple" --sdk "$sdk_path" -Xlinker -L -Xlinker "$lib_dir" "$@")
}
bin_dir=$(swift_build --show-bin-path)
rm -f "$bin_dir/MMKApp"   # SwiftPM doesn't relink when only the static library changed
swift_build

app="$out/$name.app"
echo "== $app"
rm -rf "$app" "$out/icon"
mkdir -p "$app" "$out/icon"
cp "$bin_dir/MMKApp" "$app/$name"

# icon: the macOS icon's art, full-bleed (original placeholder art drawn by a script)
swift "$root/scripts/make-icon.swift" "$out/icon/1024.png" --ios
icon() { sips -z "$1" "$1" "$out/icon/1024.png" --out "$app/$2" >/dev/null; }
icon 120 AppIcon60x60@2x.png
icon 180 AppIcon60x60@3x.png
icon 152 AppIcon76x76@2x~ipad.png
icon 167 AppIcon83.5x83.5@2x~ipad.png
rm -rf "$out/icon"

cat > "$app/Info.plist" <<EOF
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
    <key>CFBundleDevelopmentRegion</key><string>en</string>
    <key>CFBundleExecutable</key><string>$name</string>
    <key>CFBundleIdentifier</key><string>$bundle_id</string>
    <key>CFBundleInfoDictionaryVersion</key><string>6.0</string>
    <key>CFBundleName</key><string>$name</string>
    <key>CFBundleDisplayName</key><string>$name</string>
    <key>CFBundlePackageType</key><string>APPL</string>
    <key>CFBundleShortVersionString</key><string>$version</string>
    <key>CFBundleVersion</key><string>$build_number</string>
    <key>CFBundleSupportedPlatforms</key><array><string>$platform</string></array>
    <key>DTPlatformName</key><string>$sdk</string>
    <key>MinimumOSVersion</key><string>17.0</string>
    <key>UIDeviceFamily</key><array><integer>1</integer><integer>2</integer></array>
    <key>LSApplicationCategoryType</key><string>public.app-category.games</string>
    <key>CFBundleIcons</key>
    <dict><key>CFBundlePrimaryIcon</key><dict>
        <key>CFBundleIconFiles</key><array><string>AppIcon60x60</string></array>
    </dict></dict>
    <key>CFBundleIcons~ipad</key>
    <dict><key>CFBundlePrimaryIcon</key><dict>
        <key>CFBundleIconFiles</key><array><string>AppIcon60x60</string><string>AppIcon76x76</string><string>AppIcon83.5x83.5</string></array>
    </dict></dict>
    <!-- full-screen launch (no launch screen = letterboxed legacy mode) -->
    <key>UILaunchScreen</key><dict/>
    <key>UIApplicationSceneManifest</key>
    <dict><key>UIApplicationSupportsMultipleScenes</key><false/></dict>
    <key>UISupportedInterfaceOrientations</key>
    <array><string>UIInterfaceOrientationLandscapeLeft</string><string>UIInterfaceOrientationLandscapeRight</string><string>UIInterfaceOrientationPortrait</string></array>
    <key>UISupportedInterfaceOrientations~ipad</key>
    <array><string>UIInterfaceOrientationLandscapeLeft</string><string>UIInterfaceOrientationLandscapeRight</string><string>UIInterfaceOrientationPortrait</string><string>UIInterfaceOrientationPortraitUpsideDown</string></array>
    <key>UIStatusBarHidden</key><false/>
    <!-- Documents (assets/, courses/, saves) shown in the Files app: the owner copies the cache there -->
    <key>UIFileSharingEnabled</key><true/>
    <key>LSSupportsOpeningDocumentsInPlace</key><true/>
    <key>GCSupportsControllerUserInteraction</key><true/>
    <key>GCSupportedGameControllers</key>
    <array><dict><key>ProfileName</key><string>ExtendedGamepad</string></dict></array>
    <!-- 120 Hz on ProMotion iPhones (the sim runs at a fixed 60 Hz either way) -->
    <key>CADisableMinimumFrameDurationOnPhone</key><true/>
</dict>
</plist>
EOF
plutil -lint "$app/Info.plist" >/dev/null
printf 'APPL????' > "$app/PkgInfo"

if [ "$sdk" = iphonesimulator ]; then
    codesign --force --sign - --timestamp=none "$app"
elif [ -n "${MMK_IOS_SIGN_IDENTITY:-}" ] && [ -n "${MMK_IOS_PROFILE:-}" ]; then
    cp "$MMK_IOS_PROFILE" "$app/embedded.mobileprovision"
    security cms -D -i "$MMK_IOS_PROFILE" > "$out/profile.plist"
    /usr/libexec/PlistBuddy -x -c "Print :Entitlements" "$out/profile.plist" > "$out/entitlements.plist"
    codesign --force --sign "$MMK_IOS_SIGN_IDENTITY" --entitlements "$out/entitlements.plist" --timestamp=none "$app"
    rm -f "$out/profile.plist" "$out/entitlements.plist"
    codesign --verify --strict "$app"
else
    echo "note: $app is unsigned (set MMK_IOS_SIGN_IDENTITY and MMK_IOS_PROFILE to sign for a device)"
fi
echo "built $app ($version, build $build_number, $bundle_id)"
