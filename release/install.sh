#!/bin/sh
# Installs wolf for the current user, on Linux and macOS:
#   curl -fsSL https://github.com/realfamousbae/wolfpack/releases/latest/download/install.sh | sh
# It puts wolf into ~/.local/bin, adds that directory to PATH in the shell profile if needed, and
# runs `wolf setup`, which installs Node.js, TypeScript, Go and Rust into ~/.wolfpack.
# WOLFPACK_VERSION=0.1.0 installs a given release instead of the latest one.
set -eu

repo="https://github.com/realfamousbae/wolfpack/releases"
case "$(uname -s)-$(uname -m)" in
    Linux-x86_64) asset="wolfpack-linux-x64.tar.gz" ;;
    Linux-aarch64 | Linux-arm64) asset="wolfpack-linux-arm64.tar.gz" ;;
    Darwin-*) asset="wolfpack-macos-universal.tar.gz" ;;
    *)
        echo "error: there is no build of wolf for $(uname -s) on $(uname -m)" >&2
        exit 1
        ;;
esac
if [ -n "${WOLFPACK_VERSION:-}" ]; then
    url="$repo/download/v$WOLFPACK_VERSION/$asset"
else
    url="$repo/latest/download/$asset"
fi

bin="$HOME/.local/bin"
work=$(mktemp -d)
trap 'rm -rf "$work"' EXIT
echo "downloading $url"
curl -fsSL "$url" -o "$work/wolf.tar.gz"
tar -xzf "$work/wolf.tar.gz" -C "$work"
mkdir -p "$bin"
mv "$work/wolf" "$bin/wolf"
chmod 755 "$bin/wolf"
echo "installed wolf into $bin"

case ":$PATH:" in
    *":$bin:"*) ;;
    *)
        line="export PATH=\"\$HOME/.local/bin:\$PATH\""
        for profile in "$HOME/.profile" "$HOME/.bashrc" "$HOME/.zshrc"; do
            if [ -f "$profile" ] || [ "$profile" = "$HOME/.profile" ]; then
                if ! grep -qs '.local/bin' "$profile"; then
                    printf '\n# wolf\n%s\n' "$line" >>"$profile"
                    echo "added $bin to PATH in $profile"
                fi
            fi
        done
        export PATH="$bin:$PATH"
        ;;
esac

"$bin/wolf" setup || echo "some tools could not be installed; run \`wolf setup\` again later" >&2
echo
echo "wolf is installed. Open a new terminal and run: wolf new hello && cd hello && wolf run"
