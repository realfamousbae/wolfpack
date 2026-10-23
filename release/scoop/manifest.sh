#!/bin/sh
# Prints the Scoop manifest of a release: manifest.sh VERSION SUMS, where SUMS is the
# SHA256SUMS.txt of the release. The bucket realfamousbae/scoop-wolfpack keeps it in
# bucket/wolfpack.json; the workflow `packages.yml` updates it when a release is published.
set -eu
version=$1
sums=$2
sum=$(awk '$2 == "wolfpack-windows-x64.zip" || $2 == "*wolfpack-windows-x64.zip" { print $1 }' "$sums")
cat <<JSON
{
    "version": "$version",
    "description": "Statically typed language that compiles to readable TypeScript, Go and Rust",
    "homepage": "https://github.com/realfamousbae/wolfpack",
    "license": "MIT|Apache-2.0",
    "architecture": {
        "64bit": {
            "url": "https://github.com/realfamousbae/wolfpack/releases/download/v$version/wolfpack-windows-x64.zip",
            "hash": "$sum"
        }
    },
    "bin": "wolf.exe",
    "notes": "To build and run programs as TypeScript, Go or Rust, install those tools with: wolf setup",
    "checkver": {
        "github": "https://github.com/realfamousbae/wolfpack"
    },
    "autoupdate": {
        "architecture": {
            "64bit": {
                "url": "https://github.com/realfamousbae/wolfpack/releases/download/v\$version/wolfpack-windows-x64.zip"
            }
        },
        "hash": {
            "url": "\$baseurl/SHA256SUMS.txt"
        }
    }
}
JSON
