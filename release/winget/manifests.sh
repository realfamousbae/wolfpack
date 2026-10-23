#!/bin/sh
# Writes the winget manifests of a release: manifests.sh VERSION SUMS DIR, where SUMS is the
# SHA256SUMS.txt of the release. They go to DIR/manifests/r/realfamousbae/wolfpack/VERSION, the
# place of the package in microsoft/winget-pkgs. winget installs the archive as a portable
# command, without running `wolf setup` (which downloads about 1 GB).
set -eu
version=$1
sums=$2
dir="$3/manifests/r/realfamousbae/wolfpack/$version"
asset="wolfpack-windows-x64.zip"
sum=$(awk -v f="$asset" '$2 == f || $2 == "*" f { print toupper($1) }' "$sums")
id="realfamousbae.wolfpack"
mkdir -p "$dir"
cat > "$dir/$id.yaml" <<YAML
# yaml-language-server: \$schema=https://aka.ms/winget-manifest.version.1.6.0.schema.json
PackageIdentifier: $id
PackageVersion: $version
DefaultLocale: en-US
ManifestType: version
ManifestVersion: 1.6.0
YAML
cat > "$dir/$id.installer.yaml" <<YAML
# yaml-language-server: \$schema=https://aka.ms/winget-manifest.installer.1.6.0.schema.json
PackageIdentifier: $id
PackageVersion: $version
InstallerType: zip
NestedInstallerType: portable
NestedInstallerFiles:
- RelativeFilePath: wolf.exe
  PortableCommandAlias: wolf
Commands:
- wolf
Installers:
- Architecture: x64
  InstallerUrl: https://github.com/realfamousbae/wolfpack/releases/download/v$version/$asset
  InstallerSha256: $sum
ManifestType: installer
ManifestVersion: 1.6.0
YAML
cat > "$dir/$id.locale.en-US.yaml" <<YAML
# yaml-language-server: \$schema=https://aka.ms/winget-manifest.defaultLocale.1.6.0.schema.json
PackageIdentifier: $id
PackageVersion: $version
PackageLocale: en-US
Publisher: realfamousbae
PublisherUrl: https://github.com/realfamousbae
PublisherSupportUrl: https://github.com/realfamousbae/wolfpack/issues
PackageName: wolfpack
PackageUrl: https://github.com/realfamousbae/wolfpack
License: MIT OR Apache-2.0
LicenseUrl: https://github.com/realfamousbae/wolfpack#license
ShortDescription: Statically typed language that compiles to readable TypeScript, Go and Rust.
Description: |-
  wolf checks, builds, runs, tests and formats wolfpack programs. It runs them in its own virtual
  machine without other tools (wolf run --vm); to run them as TypeScript, Go or Rust, install
  those tools with: wolf setup
Moniker: wolfpack
Tags:
- compiler
- programming-language
- typescript
- go
- rust
ReleaseNotesUrl: https://github.com/realfamousbae/wolfpack/releases/tag/v$version
ManifestType: defaultLocale
ManifestVersion: 1.6.0
YAML
echo "$dir"
