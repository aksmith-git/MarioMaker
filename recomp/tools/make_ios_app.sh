#!/bin/bash
# usage: tools/make_ios_app.sh <ios | iossim> <executable> <out .app>   (make TARGET=ios-arm64 GAME=smm2 app runs it)
# The iOS / iPadOS app bundle around the runtime with the game code linked in (built on the Mac from your own dump:
# share it with nobody). Game files go in the app's Documents folder (Files app / Finder: extracted/, fonts/,
# sysdata/), not in the bundle.
#
# Signing (devices): RECOMP_IOS_IDENTITY (a codesigning identity, `security find-identity -p codesigning`) and
# RECOMP_IOS_PROFILE (a .mobileprovision for RECOMP_IOS_BUNDLE_ID that lists the device). The guest memory window
# sits at 0x7000000000-0x7320000000, far above iOS's default address space: the profile has to carry
# com.apple.developer.kernel.extended-virtual-addressing, and com.apple.developer.kernel.increased-memory-limit for the
# 3+ GB the game uses. A wildcard profile has neither: the app then installs but stops at start ("guest memory not
# reserved"). The Simulator needs no profile.
set -euo pipefail
PLAT=$1 EXE=$2 APP=$3
NAME=SMM2Recomp
BUNDLE_ID=${RECOMP_IOS_BUNDLE_ID:-org.smcraft.smm2recomp}
VERSION=3.0.3
BUILD=$(git rev-list --count HEAD 2>/dev/null || echo 1)
if [ "$PLAT" = iossim ]; then sdk=iphonesimulator; platform=iPhoneSimulator; else sdk=iphoneos; platform=iPhoneOS; fi
rm -rf "$APP"
mkdir -p "$APP"
cp "$EXE" "$APP/$NAME"
cat > "$APP/Info.plist" <<EOF
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
    <key>CFBundleDevelopmentRegion</key><string>en</string>
    <key>CFBundleExecutable</key><string>$NAME</string>
    <key>CFBundleIdentifier</key><string>$BUNDLE_ID</string>
    <key>CFBundleInfoDictionaryVersion</key><string>6.0</string>
    <key>CFBundleName</key><string>$NAME</string>
    <key>CFBundleDisplayName</key><string>SMM2 Recomp</string>
    <key>CFBundlePackageType</key><string>APPL</string>
    <key>CFBundleShortVersionString</key><string>$VERSION</string>
    <key>CFBundleVersion</key><string>$BUILD</string>
    <key>CFBundleSupportedPlatforms</key><array><string>$platform</string></array>
    <key>DTPlatformName</key><string>$sdk</string>
    <key>MinimumOSVersion</key><string>17.0</string>
    <key>UIDeviceFamily</key><array><integer>1</integer><integer>2</integer></array>
    <key>LSApplicationCategoryType</key><string>public.app-category.games</string>
    <key>UILaunchScreen</key><dict/>
    <key>UIRequiresFullScreen</key><true/>
    <key>UIStatusBarHidden</key><true/>
    <key>UIViewControllerBasedStatusBarAppearance</key><false/>
    <key>UIApplicationSupportsIndirectInputEvents</key><true/>
    <key>UIApplicationSceneManifest</key>
    <dict><key>UIApplicationSupportsMultipleScenes</key><false/></dict>
    <key>UISupportedInterfaceOrientations</key>
    <array><string>UIInterfaceOrientationLandscapeLeft</string><string>UIInterfaceOrientationLandscapeRight</string></array>
    <key>UISupportedInterfaceOrientations~ipad</key>
    <array><string>UIInterfaceOrientationLandscapeLeft</string><string>UIInterfaceOrientationLandscapeRight</string></array>
    <!-- Documents (extracted/, fonts/, sysdata/, saves, settings, caches) in the Files app and Finder -->
    <key>UIFileSharingEnabled</key><true/>
    <key>LSSupportsOpeningDocumentsInPlace</key><true/>
    <key>GCSupportsControllerUserInteraction</key><true/>
    <key>GCSupportedGameControllers</key>
    <array><dict><key>ProfileName</key><string>ExtendedGamepad</string></dict></array>
    <key>CADisableMinimumFrameDurationOnPhone</key><true/>
</dict>
</plist>
EOF
plutil -lint "$APP/Info.plist" >/dev/null
printf 'APPL????' > "$APP/PkgInfo"
# RECOMP_IOS_DATA=<data folder> (e.g. private/smm2): the game files go into the app as data/ (extracted/exefs and
# romfs, fonts, sysdata; runtime/paths.c links them into Documents), so a device needs nothing copied by hand - for
# your own device only: such an app holds the game. Staged next to the app once (rsync), then cloned into each new
# bundle (APFS clones: no second 3 GB copy).
if [ -n "${RECOMP_IOS_DATA:-}" ]; then
    stage="$(dirname "$APP")/data-stage"
    for d in extracted/exefs extracted/romfs fonts sysdata; do
        [ -d "$RECOMP_IOS_DATA/$d" ] || { echo "make_ios_app: $RECOMP_IOS_DATA/$d is missing" >&2; exit 1; }
        mkdir -p "$stage/$d"
        rsync -aL --delete "$RECOMP_IOS_DATA/$d/" "$stage/$d/"
    done
    cp -Rc "$stage" "$APP/data" 2>/dev/null || cp -R "$stage" "$APP/data"
fi
if [ "$PLAT" = iossim ]; then
    codesign --force --sign - --timestamp=none "$APP"
elif [ -n "${RECOMP_IOS_IDENTITY:-}" ] && [ -n "${RECOMP_IOS_PROFILE:-}" ]; then
    work=$(mktemp -d)
    cp "$RECOMP_IOS_PROFILE" "$APP/embedded.mobileprovision"
    security cms -D -i "$RECOMP_IOS_PROFILE" > "$work/profile.plist"
    /usr/libexec/PlistBuddy -x -c "Print :Entitlements" "$work/profile.plist" > "$work/ent.plist"
    appid=$(/usr/libexec/PlistBuddy -c "Print :Entitlements:application-identifier" "$work/profile.plist")
    team=$(/usr/libexec/PlistBuddy -c "Print :Entitlements:com.apple.developer.team-identifier" "$work/profile.plist")
    # (a wildcard profile: the app's own id in place of the wildcard)
    case "$appid" in *'*') /usr/libexec/PlistBuddy -c "Set :application-identifier $team.$BUNDLE_ID" "$work/ent.plist" ;; esac
    for k in com.apple.developer.kernel.extended-virtual-addressing com.apple.developer.kernel.increased-memory-limit; do
        /usr/libexec/PlistBuddy -c "Print :$k" "$work/ent.plist" >/dev/null 2>&1 || echo "make_ios_app: the profile lacks $k" >&2
    done
    codesign --force --sign "$RECOMP_IOS_IDENTITY" --entitlements "$work/ent.plist" --timestamp=none "$APP"
    rm -rf "$work"
    codesign --verify --strict "$APP" && echo "make_ios_app: signed $APP ($BUNDLE_ID)"
else
    echo "make_ios_app: $APP left unsigned (set RECOMP_IOS_IDENTITY and RECOMP_IOS_PROFILE)" >&2
fi
du -sh "$APP"
