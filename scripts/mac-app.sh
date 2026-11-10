#!/usr/bin/env bash
# macOS: builds target/Polymoire.app (ad-hoc signed, with the Info.plist that declares
# system-audio capture) and opens it. macOS only lets a real app bundle ask for
# "System Audio Recording" permission; a bare binary run from a terminal gets silence.
# Any KEY=VALUE arguments are passed to the app as environment variables.
set -euo pipefail
cd "$(dirname "$0")/.."
if [[ "$(uname)" != "Darwin" ]]; then
  echo "mac-app.sh only runs on macOS"; exit 1
fi
cargo build --release
app=target/Polymoire.app
mkdir -p "$app/Contents/MacOS"
cp target/release/polymoire "$app/Contents/MacOS/Polymoire"
version=$(sed -n 's/^version = "\(.*\)"/\1/p' Cargo.toml | head -1)
cat > "$app/Contents/Info.plist" <<PLIST
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
	<key>CFBundleExecutable</key><string>Polymoire</string>
	<key>CFBundleIdentifier</key><string>app.polymoire.visualizer</string>
	<key>CFBundleName</key><string>Polymoire</string>
	<key>CFBundlePackageType</key><string>APPL</string>
	<key>CFBundleShortVersionString</key><string>${version}</string>
	<key>CFBundleVersion</key><string>${version}</string>
	<key>LSMinimumSystemVersion</key><string>11.0</string>
	<key>NSHighResolutionCapable</key><true/>
	<key>NSAudioCaptureUsageDescription</key>
	<string>Polymoire listens to what your Mac is playing so the visuals can move to the music. Nothing is recorded or saved.</string>
</dict>
</plist>
PLIST
codesign --force --sign - "$app"
env_args=()
for kv in "$@"; do env_args+=(--env "$kv"); done
out="$PWD/target/app-stdout.txt"
: > "$out"
open -W -n ${env_args[@]+"${env_args[@]}"} --stdout "$out" --stderr "$PWD/target/app-stderr.txt" "$app"
cat "$out"
