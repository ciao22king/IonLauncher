#!/usr/bin/env bash
# Builds a portable AppImage from an already-built ionlauncher binary.
#
# Usage: installer/linux/build-appimage.sh [binary] [output-dir]
#
# The AppImage bundles the binary, the desktop entry and the icons into one
# executable file that runs on any glibc distribution, with no install step
# and no root. appimagetool is downloaded on first use and cached in the
# cargo target directory.
set -euo pipefail

root=$(cd "$(dirname "$0")/../.." && pwd)
binary=${1:-$root/target/release/ionlauncher}
outdir=${2:-$root/dist}

if [ ! -f "$binary" ]; then
  echo "no binary at $binary — run 'cargo build --release' first" >&2
  exit 1
fi

version=$(sed -n 's/^version *= *"\([^"]*\)".*/\1/p' "$root/Cargo.toml" | head -1)
name=ionlauncher
arch=x86_64

tools=${APPIMAGE_TOOLS_DIR:-$root/target/appimage-tools}
mkdir -p "$tools"

appimagetool=$tools/appimagetool
if [ ! -x "$appimagetool" ]; then
  echo "downloading appimagetool into $tools" >&2
  url=https://github.com/AppImage/appimagetool/releases/download/continuous/appimagetool-$arch.AppImage
  curl -fsSL -o "$appimagetool" "$url"
  chmod +x "$appimagetool"
fi

appdir=$(mktemp -d)
trap 'rm -rf "$appdir"' EXIT

install -Dm755 "$binary" "$appdir/usr/bin/$name"
install -Dm644 "$root/installer/linux/$name.desktop" "$appdir/$name.desktop"
install -Dm644 "$root/assets/linux/ionlauncher-256.png" "$appdir/$name.png"
install -Dm644 "$root/assets/ionlauncher.svg" "$appdir/usr/share/icons/hicolor/scalable/apps/$name.svg"

for png in "$root"/assets/linux/ionlauncher-*.png; do
  size=$(basename "$png" | sed 's/ionlauncher-\([0-9]*\)\.png/\1/')
  install -Dm644 "$png" "$appdir/usr/share/icons/hicolor/${size}x${size}/apps/$name.png"
done

# AppRun is the entry point AppImage runs; the symlink form is the documented
# convention and lets the bundled binary find its own icons.
ln -sf usr/bin/$name "$appdir/AppRun"

# appimagetool shells out to the system desktop-file-validate when present.
# On a bare CI image it is not, so tell it not to insist.
export ARCH=$arch
mkdir -p "$outdir"
out="$outdir/${name}-${version}-${arch}.AppImage"
APPIMAGE_EXTRACT_AND_RUN=1 "$appimagetool" --no-appstream "$appdir" "$out" >&2

echo "$out"
