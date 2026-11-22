#!/usr/bin/env bash
set -euo pipefail

repo=$(cd -- "$(dirname -- "$0")/../.." && pwd)
cd "$repo"
case "${1:-simulator}" in
    simulator) target=aarch64-apple-ios-sim; sdk=iphonesimulator; platform=iPhoneSimulator ;;
    device) target=aarch64-apple-ios; sdk=iphoneos; platform=iPhoneOS ;;
    *) printf 'Usage: %s [simulator|device]\n' "$0" >&2; exit 2 ;;
esac
if [[ $# -gt 0 ]]; then shift; fi

npm --prefix native-shell run page:build
export CARGO_TARGET_DIR="${CARGO_TARGET_DIR:-$repo/native-shell/ios/build/rust}"
sdk_path=$(/usr/bin/xcrun --sdk "$sdk" --show-sdk-path)
app_id=io.github.kaminarios.govue.nsh
cargo_command=build
link_args=()
if [[ "$sdk" == iphonesimulator ]]; then
    # The simulator reads Xcode-style simulated entitlements from the executable.
    entitlement_dir="$repo/native-shell/ios/build/$sdk"
    mkdir -p "$entitlement_dir"
    entitlements="$entitlement_dir/GoVue.xcent"
    cat > "$entitlements" <<EOF
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
    <key>application-identifier</key><string>GOVUESIM01.$app_id</string>
</dict>
</plist>
EOF
    /usr/bin/derq query --format xml --input "$entitlements" --output "$entitlements.der" --raw
    cargo_command=rustc
    link_args=(--
        -C link-arg=-Xlinker -C link-arg=-sectcreate
        -C link-arg=-Xlinker -C link-arg=__TEXT
        -C link-arg=-Xlinker -C link-arg=__entitlements
        -C link-arg=-Xlinker -C "link-arg=$entitlements"
        -C link-arg=-Xlinker -C link-arg=-sectcreate
        -C link-arg=-Xlinker -C link-arg=__TEXT
        -C link-arg=-Xlinker -C link-arg=__ents_der
        -C link-arg=-Xlinker -C "link-arg=$entitlements.der")
fi
env -u CC -u CXX -u CFLAGS -u CPPFLAGS -u LDFLAGS -u MACOSX_DEPLOYMENT_TARGET \
    SDKROOT="$sdk_path" IPHONEOS_DEPLOYMENT_TARGET=16.0 \
    cargo "$cargo_command" --locked --release --manifest-path native-shell/app/Cargo.toml \
        --target "$target" --bin GoVue "$@" "${link_args[@]}"

version=$(node -p 'require("./package.json").version')
bundle="$repo/native-shell/ios/build/$sdk/GoVue.app"
mkdir -p "$bundle"
install -m 755 "$CARGO_TARGET_DIR/$target/release/GoVue" "$bundle/GoVue"
install -m 644 assets/icon.png "$bundle/AppIcon.png"
cat > "$bundle/Info.plist" <<EOF
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
    <key>CFBundleDevelopmentRegion</key><string>en</string>
    <key>CFBundleDisplayName</key><string>GoVue</string>
    <key>CFBundleExecutable</key><string>GoVue</string>
    <key>CFBundleIconFiles</key><array><string>AppIcon</string></array>
    <key>CFBundleIdentifier</key><string>$app_id</string>
    <key>CFBundleInfoDictionaryVersion</key><string>6.0</string>
    <key>CFBundleName</key><string>GoVue</string>
    <key>CFBundlePackageType</key><string>APPL</string>
    <key>CFBundleShortVersionString</key><string>$version</string>
    <key>CFBundleSupportedPlatforms</key><array><string>$platform</string></array>
    <key>CFBundleVersion</key><string>$version</string>
    <key>LSRequiresIPhoneOS</key><true/>
    <key>MinimumOSVersion</key><string>16.0</string>
    <key>UIDeviceFamily</key><array><integer>1</integer><integer>2</integer></array>
    <key>UILaunchScreen</key><dict/>
    <key>UIApplicationSceneManifest</key><dict><key>UIApplicationSupportsMultipleScenes</key><false/></dict>
    <key>NSCameraUsageDescription</key><string>Allow GoVue to scan provider QR codes and take photos for chat messages.</string>
    <key>NSMicrophoneUsageDescription</key><string>Allow GoVue to use the microphone for voice input in chat.</string>
    <key>NSSpeechRecognitionUsageDescription</key><string>Allow GoVue to turn your speech into chat messages.</string>
    <key>UIBackgroundModes</key><array><string>audio</string></array>
    <key>UISupportedInterfaceOrientations</key>
    <array>
        <string>UIInterfaceOrientationPortrait</string>
        <string>UIInterfaceOrientationLandscapeLeft</string>
        <string>UIInterfaceOrientationLandscapeRight</string>
    </array>
    <key>UISupportedInterfaceOrientations~ipad</key>
    <array>
        <string>UIInterfaceOrientationPortrait</string>
        <string>UIInterfaceOrientationPortraitUpsideDown</string>
        <string>UIInterfaceOrientationLandscapeLeft</string>
        <string>UIInterfaceOrientationLandscapeRight</string>
    </array>
    <key>NSAppTransportSecurity</key><dict><key>NSAllowsLocalNetworking</key><true/></dict>
</dict>
</plist>
EOF
/usr/bin/plutil -lint "$bundle/Info.plist"
if [[ "$sdk" == iphonesimulator ]]; then
    /usr/bin/codesign --force --sign - "$bundle"
fi
printf '%s\n' "$bundle"
