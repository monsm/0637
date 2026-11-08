#!/usr/bin/env bash
# Build and install a trivial app on the given simulator. This is a control
# experiment: if even this app fails to install with "Missing bundle ID", the
# problem is the runner's Xcode/simulator combination, not our project.
set -uo pipefail

UDID="${1:?simulator udid required}"
WORK="$(mktemp -d)"
mkdir -p "$WORK/App"
cat > "$WORK/App/AppDelegate.swift" <<'SWIFT'
import UIKit

@main final class AppDelegate: UIResponder, UIApplicationDelegate {
    var window: UIWindow?
    func application(_ app: UIApplication,
                     didFinishLaunchingWithOptions options: [UIApplication.LaunchOptionsKey: Any]?) -> Bool {
        true
    }
}
SWIFT
cat > "$WORK/project.yml" <<'YML'
name: Control
options:
  bundleIdPrefix: test.control
targets:
  Control:
    type: application
    platform: iOS
    sources: [App]
    settings:
      base:
        PRODUCT_BUNDLE_IDENTIFIER: test.control.app
        GENERATE_INFOPLIST_FILE: YES
        ENABLE_DEBUG_DYLIB: NO
YML

cd "$WORK"
if ! xcodegen generate > control-gen.log 2>&1; then
  echo "control: xcodegen failed"; tail -20 control-gen.log; exit 2
fi
if ! xcodebuild -project Control.xcodeproj -scheme Control -configuration Debug \
  -sdk iphonesimulator -arch arm64 -derivedDataPath "$WORK/build" \
  CODE_SIGN_IDENTITY=- CODE_SIGNING_REQUIRED=NO CODE_SIGNING_ALLOWED=YES \
  build > control-build.log 2>&1; then
  echo "control: xcodebuild failed"; tail -40 control-build.log; exit 3
fi

APP="$WORK/build/Build/Products/Debug-iphonesimulator/Control.app"
echo "control bundle contents:"
ls -la "$APP" | head -8
echo "control signature:"
/usr/bin/codesign -dv "$APP" 2>&1 | head -8
if xcrun simctl install "$UDID" "$APP"; then
  echo "CONTROL-INSTALL-OK"
else
  echo "CONTROL-INSTALL-FAILED"
fi
