#!/bin/sh
set -eu
cd "$(dirname "$0")/.."

profile="${1:-release}"
case "$profile" in
    release) cargo build --release --locked ;;
    debug) cargo build --locked ;;
    *) printf 'Usage: sh scripts/macos-app.sh [release|debug]\n' >&2; exit 1 ;;
esac

app="dist/Flexspeak.app"
mkdir -p "$app/Contents/MacOS" "$app/Contents/Resources"
cp assets/lucide/LICENSE "$app/Contents/Resources/Lucide-LICENSE.txt"
cp assets/webrtc/LICENSE.md "$app/Contents/Resources/WebRTC-LICENSE.txt"
cp assets/webrtc/NOTICE.txt "$app/Contents/Resources/WebRTC-NOTICE.txt"
cp vendor/tsclientlib/LICENSE-MIT "$app/Contents/Resources/tsclientlib-LICENSE-MIT.txt"
cp vendor/tsclientlib/LICENSE-APACHE "$app/Contents/Resources/tsclientlib-LICENSE-APACHE.txt"
# Preserve the mapped executable of an already-running voice session.
cp "target/$profile/flexspeak" "$app/Contents/MacOS/flexspeak.new"
mv -f "$app/Contents/MacOS/flexspeak.new" "$app/Contents/MacOS/flexspeak"
# The Icon Composer source compiles to Assets.car (Liquid Glass on macOS 26) and an .icns for
# earlier systems. Without an actool that reads .icon files, use the checked-in .icns.
if ! xcrun actool assets/icon/Flexspeak.icon --compile "$app/Contents/Resources" \
    --output-format human-readable-text --errors --app-icon Flexspeak --include-all-app-icons \
    --output-partial-info-plist "$app/Contents/Resources/.icon-info.plist" \
    --enable-on-demand-resources NO --development-region en --target-device mac \
    --minimum-deployment-target 14.2 --platform macosx >/dev/null 2>&1; then
    echo "actool could not compile assets/icon/Flexspeak.icon; using assets/icon/Flexspeak.icns" >&2
    rm -f "$app/Contents/Resources/Assets.car"
    cp assets/icon/Flexspeak.icns "$app/Contents/Resources/Flexspeak.icns"
fi
rm -f "$app/Contents/Resources/.icon-info.plist"
cat > "$app/Contents/Info.plist" <<'PLIST'
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0"><dict>
<key>CFBundleExecutable</key><string>flexspeak</string>
<key>CFBundleIconFile</key><string>Flexspeak</string>
<key>CFBundleIconName</key><string>Flexspeak</string>
<key>CFBundleIdentifier</key><string>io.venator.flexspeak</string>
<key>CFBundleName</key><string>Flexspeak</string>
<key>CFBundleDisplayName</key><string>Flexspeak</string>
<key>CFBundlePackageType</key><string>APPL</string>
<key>CFBundleShortVersionString</key><string>0.1.0</string>
<key>CFBundleVersion</key><string>1</string>
<key>LSMinimumSystemVersion</key><string>14.2</string>
<key>NSHighResolutionCapable</key><true/>
<key>NSMicrophoneUsageDescription</key><string>Flexspeak uses your microphone for TeamSpeak voice chat and the local microphone test.</string>
<key>NSScreenCaptureUsageDescription</key><string>Flexspeak shares the screen or window you select with viewers you approve.</string>
<key>NSAudioCaptureUsageDescription</key><string>Flexspeak shares sound from your chosen output device when you enable system audio for a screen share.</string>
</dict></plist>
PLIST
if [ -n "${NATIVE_VOICE_BUILD_NUMBER:-}" ]; then
    /usr/libexec/PlistBuddy -c "Set :CFBundleVersion $NATIVE_VOICE_BUILD_NUMBER" "$app/Contents/Info.plist"
fi
if [ -n "${NATIVE_VOICE_APP_VERSION:-}" ]; then
    /usr/libexec/PlistBuddy -c "Set :CFBundleShortVersionString $NATIVE_VOICE_APP_VERSION" "$app/Contents/Info.plist"
fi
codesign --force --sign - "$app"
printf 'Created %s/%s\n' "$PWD" "$app"
