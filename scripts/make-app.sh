#!/bin/sh
# Build the Rust core + Swift app in release mode and assemble build/<Name>.app (ad-hoc signed).
#   scripts/make-app.sh            → build/SMCraft.app
#   open build/SMCraft.app
# The app name, bundle id and version come from app/Sources/MMKApp/Identity.swift (the one place
# to rename the app).
set -eu
root=$(cd "$(dirname "$0")/.." && pwd)
identity="$root/app/Sources/MMKApp/Identity.swift"
field() { sed -n "s/^ *static let $1 = \"\(.*\)\".*/\1/p" "$identity"; }
name=$(field name)
bundle_id=$(field bundleID)
version=$(field version)
[ -n "$name" ] && [ -n "$bundle_id" ] && [ -n "$version" ] || { echo "cannot read $identity" >&2; exit 1; }
build_number=$(git -C "$root" rev-list --count HEAD 2>/dev/null || echo 1)

echo "== Rust core"
(cd "$root/core" && cargo build --release -p mmk-ffi)
echo "== Swift app"
# SwiftPM doesn't relink when only the static library changed
rm -f "$root/app/.build/release/MMKApp"
(cd "$root/app" && swift build -c release)

out="$root/build"
app="$out/$name.app"
echo "== $app"
rm -rf "$app" "$out/icon.iconset"
mkdir -p "$app/Contents/MacOS" "$app/Contents/Resources" "$out/icon.iconset"
cp "$root/app/.build/release/MMKApp" "$app/Contents/MacOS/$name"

# icon: original placeholder art drawn by a script → .icns
swift "$root/scripts/make-icon.swift" "$out/icon_1024.png"
for s in 16 32 128 256 512; do
    sips -z $s $s "$out/icon_1024.png" --out "$out/icon.iconset/icon_${s}x${s}.png" >/dev/null
    d=$((s * 2))
    sips -z $d $d "$out/icon_1024.png" --out "$out/icon.iconset/icon_${s}x${s}@2x.png" >/dev/null
done
iconutil -c icns "$out/icon.iconset" -o "$app/Contents/Resources/AppIcon.icns"
rm -rf "$out/icon.iconset"

cat > "$app/Contents/Info.plist" <<EOF
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
    <key>CFBundleDevelopmentRegion</key><string>en</string>
    <key>CFBundleExecutable</key><string>$name</string>
    <key>CFBundleIconFile</key><string>AppIcon</string>
    <key>CFBundleIdentifier</key><string>$bundle_id</string>
    <key>CFBundleInfoDictionaryVersion</key><string>6.0</string>
    <key>CFBundleName</key><string>$name</string>
    <key>CFBundleDisplayName</key><string>$name</string>
    <key>CFBundlePackageType</key><string>APPL</string>
    <key>CFBundleShortVersionString</key><string>$version</string>
    <key>CFBundleVersion</key><string>$build_number</string>
    <key>LSApplicationCategoryType</key><string>public.app-category.games</string>
    <key>LSMinimumSystemVersion</key><string>14.0</string>
    <key>NSHighResolutionCapable</key><true/>
    <key>NSSupportsAutomaticGraphicsSwitching</key><true/>
    <key>GCSupportsControllerUserInteraction</key><true/>
</dict>
</plist>
EOF
plutil -lint "$app/Contents/Info.plist" >/dev/null

codesign --force --sign - --timestamp=none "$app"
codesign --verify --strict "$app"
echo "built $app ($version, build $build_number)"
