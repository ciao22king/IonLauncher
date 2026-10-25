#!/usr/bin/env bash
# Builds IonLauncher.app and a drag-to-install .dmg. Must run on macOS:
# it uses hdiutil, codesign and lipo, which have no Linux equivalent.
#
# Usage: installer/macos/build-macos.sh [binary] [output-dir]
#
# The binary defaults to the release build for the host architecture. Pass a
# second architecture's binary with UNIVERSAL_SECOND_BINARY=<path> to lipo the
# two into one universal build.
set -euo pipefail

root=$(cd "$(dirname "$0")/../.." && pwd)
binary=${1:-$root/target/release/ionlauncher}
outdir=${2:-$root/dist}
second=${UNIVERSAL_SECOND_BINARY:-}

if [ "$(uname -s)" != "Darwin" ]; then
  echo "this script needs macOS (hdiutil, codesign, lipo)" >&2
  exit 1
fi
if [ ! -f "$binary" ]; then
  echo "no binary at $binary — run 'cargo build --release' first" >&2
  exit 1
fi

version=$(sed -n 's/^version *= *"\([^"]*\)".*/\1/p' "$root/Cargo.toml" | head -1)
if [ -z "$version" ]; then
  echo "could not read the version from Cargo.toml" >&2
  exit 1
fi

name=IonLauncher
bundle_id=io.github.ciao22king.IonLauncher
app="$outdir/$name.app"

rm -rf "$app"
mkdir -p "$app/Contents/MacOS" "$app/Contents/Resources"

install -m755 "$binary" "$app/Contents/MacOS/ionlauncher"

# A universal build has to be produced after both slices exist, so lipo runs
# here rather than at link time.
if [ -n "$second" ]; then
  if [ ! -f "$second" ]; then
    echo "no second binary at $second" >&2
    exit 1
  fi
  lipo -create -output "$app/Contents/MacOS/ionlauncher" "$binary" "$second"
  chmod 755 "$app/Contents/MacOS/ionlauncher"
fi

install -m644 "$root/assets/ionlauncher.icns" "$app/Contents/Resources/ionlauncher.icns"

sed -e "s/@VERSION@/$version/g" -e "s/@BUNDLE_ID@/$bundle_id/g" \
  "$root/installer/macos/Info.plist.in" > "$app/Contents/Info.plist"

# Four-character type and creator codes. APPL is a plain application; the
# creator is conventionally '????' when there is no signature.
printf 'APPL????' > "$app/Contents/PkgInfo"

# Ad-hoc signing is enough to launch locally and keeps Gatekeeper's
# "damaged" dialog away for a download. Notarisation needs an Apple
# Developer ID, which this project does not have.
codesign --force --deep --sign - "$app" >/dev/null 2>&1 \
  || echo "warning: ad-hoc codesign failed; the app still runs locally" >&2

plutil -lint "$app/Contents/Info.plist"

# Build the dmg from a staging directory so the window shows the app next to
# an /Applications symlink, the layout people expect for drag-install.
stage=$(mktemp -d)
trap 'rm -rf "$stage"' EXIT
cp -R "$app" "$stage/"
ln -s /Applications "$stage/Applications"

dmg="$outdir/$name-$version.dmg"
rm -f "$dmg"
hdiutil create -volname "$name" -srcfolder "$stage" -ov -format UDZO "$dmg" >/dev/null

echo "$app"
echo "$dmg"
