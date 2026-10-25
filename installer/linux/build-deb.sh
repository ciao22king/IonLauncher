#!/usr/bin/env bash
# Builds a Debian package from an already-built ionlauncher binary.
#
# Usage: installer/linux/build-deb.sh [binary] [output-dir]
#
# The binary defaults to the release build, the output directory to dist/.
# Nothing is compiled here: run `cargo build --release` first.
set -euo pipefail

root=$(cd "$(dirname "$0")/../.." && pwd)
binary=${1:-$root/target/release/ionlauncher}
outdir=${2:-$root/dist}

if [ ! -f "$binary" ]; then
  echo "no binary at $binary — run 'cargo build --release' first" >&2
  exit 1
fi

version=$(sed -n 's/^version *= *"\([^"]*\)".*/\1/p' "$root/Cargo.toml" | head -1)
if [ -z "$version" ]; then
  echo "could not read the version from Cargo.toml" >&2
  exit 1
fi

# amd64 is the Debian name for the x86_64 build this script packages.
arch=amd64
name=ionlauncher

# dpkg needs root:root ownership regardless of who runs the build.
stage=$(mktemp -d)
trap 'rm -rf "$stage"' EXIT
# mktemp makes the directory 0700; the package root must be readable.
chmod 755 "$stage"

install -Dm755 "$binary" "$stage/usr/bin/$name"
install -Dm644 "$root/installer/linux/$name.desktop" \
  "$stage/usr/share/applications/$name.desktop"

# hicolor is the shared icon theme: one PNG per size so every shell and menu
# picks the right one instead of scaling a single large image down.
for png in "$root"/assets/linux/ionlauncher-*.png; do
  size=$(basename "$png" | sed 's/ionlauncher-\([0-9]*\)\.png/\1/')
  install -Dm644 "$png" "$stage/usr/share/icons/hicolor/${size}x${size}/apps/$name.png"
done

install -Dm644 "$root/LICENSE" "$stage/usr/share/doc/$name/copyright"

# Debian policy wants an extended description indented by one space.
mkdir -p "$stage/DEBIAN"
cat > "$stage/DEBIAN/control" <<EOF
Package: $name
Version: $version
Section: games
Priority: optional
Architecture: $arch
Maintainer: IonLauncher <ciao22king@users.noreply.github.com>
Homepage: https://github.com/ciao22king/IonLauncher
Depends: libc6, libgcc-s1, libgl1, libegl1, libx11-6, libx11-xcb1, libxcursor1,
 libxrender1, libxi6, libxkbcommon0, libxkbcommon-x11-0, libwayland-client0,
 libwayland-egl1
Description: Unified launcher for Steam, Epic Games and GOG
 IonLauncher reads the game libraries that Steam, Epic Games and GOG
 already keep on your machine and shows them in one place. It launches
 games through their own store client, so cloud saves, achievements and
 overlays keep working.
 .
 It has no account and no telemetry, and it does not touch your game
 installations.
EOF

mkdir -p "$outdir"
deb="$outdir/${name}_${version}_${arch}.deb"
dpkg-deb --build --root-owner-group "$stage" "$deb" >/dev/null

echo "$deb"
