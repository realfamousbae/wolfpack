#!/bin/sh
# Builds `wolfpack-<version>-macos.pkg` from a universal `wolf` binary.
# Usage: release/macos/build-pkg.sh <version> <wolf binary> <output directory>
set -e
version=$1
binary=$2
out=$3
here=$(cd "$(dirname "$0")" && pwd)
work=$(mktemp -d)
mkdir -p "$work/root/usr/local/bin" "$out"
cp "$binary" "$work/root/usr/local/bin/wolf"
chmod 755 "$work/root/usr/local/bin/wolf"
# No extended attributes, so the payload has no AppleDouble `._` files.
xattr -cr "$work/root" 2>/dev/null || true
export COPYFILE_DISABLE=1
pkgbuild --root "$work/root" --scripts "$here/scripts" \
    --identifier io.github.realfamousbae.wolfpack --version "$version" --install-location / \
    "$work/wolf.pkg"
sed "s/@VERSION@/$version/g" "$here/distribution.xml" > "$work/distribution.xml"
productbuild --distribution "$work/distribution.xml" --resources "$here/resources" \
    --package-path "$work" "$out/wolfpack-$version-macos.pkg"
rm -rf "$work"
