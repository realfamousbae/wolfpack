#!/bin/sh
# Installs wolf for the current user, on Linux and macOS:
#
#   curl -fsSL https://github.com/realfamousbae/wolfpack/releases/latest/download/install.sh | sh
#
# It downloads the archive of the release for this machine, checks it against SHA256SUMS.txt of the
# same release, puts wolf into ~/.local/bin, adds that directory to PATH in the shell profiles if
# needed, and runs `wolf setup`, which installs Node.js, TypeScript, Go and Rust into ~/.wolfpack.
#
# Options (after `sh -s --` when piped, for example `... | sh -s -- --no-setup`), or the
# environment variables next to them:
#
#   --version <x.y.z>   WOLFPACK_VERSION         install this release instead of the latest one
#   --dir <path>        WOLFPACK_INSTALL_DIR     install wolf into this directory
#   --no-modify-path    WOLFPACK_NO_MODIFY_PATH  do not change shell profiles
#   --no-setup          WOLFPACK_NO_SETUP        do not run `wolf setup`
#                       WOLFPACK_DOWNLOAD_URL    take the files from here (a mirror, or a local
#                                                directory with file://) instead of GitHub
#                       WOLFPACK_SKIP_CHECKSUM   do not check the download (when no SHA-256
#                                                tool is available)
#
# The download honours the usual proxy variables (https_proxy, HTTPS_PROXY, no_proxy).
#
# Everything is inside main, so a download of this script that breaks off halfway runs nothing.

set -eu

releases="https://github.com/realfamousbae/wolfpack/releases"
# The Linux builds are made on Ubuntu 22.04 and need this glibc or a newer one.
min_glibc="2.35"

say() {
    printf 'wolfpack: %s\n' "$1"
}

fail() {
    printf 'wolfpack: error: %s\n' "$1" >&2
    exit 1
}

usage() {
    cat <<'EOF'
Installs wolf for the current user.

usage: install.sh [--version <x.y.z>] [--dir <path>] [--no-modify-path] [--no-setup]

  --version <x.y.z>   install this release instead of the latest one
  --dir <path>        install wolf into this directory (default ~/.local/bin)
  --no-modify-path    do not change shell profiles
  --no-setup          do not run `wolf setup`
EOF
}

has() {
    command -v "$1" >/dev/null 2>&1
}

need() {
    has "$1" || fail "this installer needs \`$1\`, which is not installed; install it and try again"
}

# Downloads $1 into the file $2, with retries, with curl or with wget.
download() {
    case "$1" in
        file://*)
            cp "${1#file://}" "$2" || fail "cannot copy $1"
            return
            ;;
    esac
    if has curl; then
        curl --proto '=https' --tlsv1.2 --fail --silent --show-error --location \
            --retry 3 --retry-delay 2 --connect-timeout 20 \
            --output "$2" "$1" && return
    elif has wget; then
        wget --https-only --tries=3 --waitretry=2 --timeout=20 --quiet \
            --output-document="$2" "$1" && return
    else
        fail "neither curl nor wget is installed; install one of them and try again"
    fi
    fail "cannot download $1 (check the network connection, a proxy, or the version number)"
}

# Prints the SHA-256 sum of the file $1, or nothing if no tool for it is installed.
sha256() {
    if has sha256sum; then
        sha256sum "$1" | cut -d ' ' -f 1
    elif has shasum; then
        shasum -a 256 "$1" | cut -d ' ' -f 1
    elif has openssl; then
        openssl dgst -sha256 "$1" | sed 's/^.*= *//'
    fi
}

# Succeeds if the version $1 is at least the version $2 (both MAJOR.MINOR).
version_at_least() {
    have_major=${1%%.*}
    have_minor=${1#*.}
    have_minor=${have_minor%%.*}
    want_major=${2%%.*}
    want_minor=${2#*.}
    [ "$have_major" -gt "$want_major" ] ||
        { [ "$have_major" -eq "$want_major" ] && [ "$have_minor" -ge "$want_minor" ]; }
}

# Prints the name of the archive for this machine, or stops with the reason why there is none.
pick_asset() {
    os=$(uname -s)
    arch=$(uname -m)
    case "$os" in
        Darwin)
            echo "wolfpack-macos-universal.tar.gz"
            return
            ;;
        Linux) ;;
        MINGW* | MSYS* | CYGWIN* | Windows_NT)
            fail "this is Windows; run in PowerShell: irm $releases/latest/download/install.ps1 | iex"
            ;;
        *)
            fail "there is no build of wolf for $os; build it from source: cargo install --locked wolfpack-lang"
            ;;
    esac

    # glibc first: a glibc system may have the musl package installed too.
    glibc=$(getconf GNU_LIBC_VERSION 2>/dev/null | sed -n 's/^glibc \([0-9]*\.[0-9]*\).*/\1/p')
    if [ -n "$glibc" ]; then
        if ! version_at_least "$glibc" "$min_glibc"; then
            fail "wolf needs glibc $min_glibc or newer, this system has $glibc; update the system or build wolf from source"
        fi
    elif ls /lib/ld-musl-* >/dev/null 2>&1 || { has ldd && ldd --version 2>&1 | grep -qi musl; }; then
        fail "this Linux uses musl (Alpine?); the builds of wolf need glibc. Build it from source: cargo install --locked wolfpack-lang"
    fi

    case "$arch" in
        x86_64 | amd64) echo "wolfpack-linux-x64.tar.gz" ;;
        aarch64 | arm64) echo "wolfpack-linux-arm64.tar.gz" ;;
        *) fail "there is no build of wolf for Linux on $arch; build it from source: cargo install --locked wolfpack-lang" ;;
    esac
}

# Appends the line $2 to the file $1 unless the file already has it.
add_line() {
    if [ -f "$1" ] && grep -qsF "$2" "$1"; then
        return 1
    fi
    mkdir -p "$(dirname "$1")"
    printf '\n# Added by the wolfpack installer.\n%s\n' "$2" >>"$1"
}

# Puts the directory $1 on PATH for future shells: sh, bash and zsh profiles, and fish.
add_to_path() {
    dir=$1
    case ":${PATH:-}:" in
        *":$dir:"*) return ;;
    esac
    # Written with $HOME when possible, so the line still works if the home directory moves.
    case "$dir" in
        "$HOME"/*) shown="\$HOME/${dir#"$HOME"/}" ;;
        *) shown="$dir" ;;
    esac
    line="export PATH=\"$shown:\$PATH\""
    changed=""
    for profile in "$HOME/.profile" "$HOME/.bashrc" "$HOME/.bash_profile" "${ZDOTDIR:-$HOME}/.zshrc" "${ZDOTDIR:-$HOME}/.zprofile"; do
        # .profile is created if missing; the others only if the shell already uses them.
        if [ -f "$profile" ] || [ "$profile" = "$HOME/.profile" ]; then
            if add_line "$profile" "$line"; then
                changed="$changed $profile"
            fi
        fi
    done
    # zsh on macOS reads no .profile; create .zshrc when zsh is the login shell.
    case "${SHELL:-}" in
        */zsh)
            zshrc="${ZDOTDIR:-$HOME}/.zshrc"
            if [ ! -f "$zshrc" ] && add_line "$zshrc" "$line"; then
                changed="$changed $zshrc"
            fi
            ;;
    esac
    if has fish || [ -d "$HOME/.config/fish" ]; then
        fish_conf="${XDG_CONFIG_HOME:-$HOME/.config}/fish/conf.d/wolfpack.fish"
        if add_line "$fish_conf" "fish_add_path --global \"$dir\""; then
            changed="$changed $fish_conf"
        fi
    fi
    if [ -n "$changed" ]; then
        say "added $dir to PATH in:$changed"
    fi
}

main() {
    version="${WOLFPACK_VERSION:-}"
    dir="${WOLFPACK_INSTALL_DIR:-}"
    modify_path=1
    run_setup=1
    [ -z "${WOLFPACK_NO_MODIFY_PATH:-}" ] || modify_path=""
    [ -z "${WOLFPACK_NO_SETUP:-}" ] || run_setup=""

    while [ $# -gt 0 ]; do
        case "$1" in
            --version)
                [ $# -ge 2 ] || fail "--version needs a value, for example --version 0.1.0"
                version=$2
                shift 2
                ;;
            --version=*)
                version=${1#--version=}
                shift
                ;;
            --dir)
                [ $# -ge 2 ] || fail "--dir needs a value"
                dir=$2
                shift 2
                ;;
            --dir=*)
                dir=${1#--dir=}
                shift
                ;;
            --no-modify-path)
                modify_path=""
                shift
                ;;
            --no-setup)
                run_setup=""
                shift
                ;;
            -h | --help)
                usage
                exit 0
                ;;
            *) fail "unknown option $1 (see --help)" ;;
        esac
    done

    [ -n "${HOME:-}" ] || fail "HOME is not set"
    need uname
    need tar
    need gzip
    need mktemp
    version=${version#v}
    [ -n "$dir" ] || dir="$HOME/.local/bin"

    asset=$(pick_asset)
    if [ -n "${WOLFPACK_DOWNLOAD_URL:-}" ]; then
        base=${WOLFPACK_DOWNLOAD_URL%/}
    elif [ -n "$version" ]; then
        base="$releases/download/v$version"
    else
        base="$releases/latest/download"
    fi

    umask 022
    work=$(mktemp -d 2>/dev/null || mktemp -d -t wolfpack)
    trap 'rm -rf "$work"' EXIT
    trap 'exit 130' INT TERM

    say "downloading $base/$asset"
    download "$base/$asset" "$work/$asset"

    if [ -n "${WOLFPACK_SKIP_CHECKSUM:-}" ]; then
        say "not checking the download (WOLFPACK_SKIP_CHECKSUM is set)"
    else
        download "$base/SHA256SUMS.txt" "$work/SHA256SUMS.txt"
        want=$(awk -v name="$asset" '$2 == name || $2 == "*" name { print $1 }' "$work/SHA256SUMS.txt")
        [ -n "$want" ] || fail "SHA256SUMS.txt of the release does not list $asset"
        got=$(sha256 "$work/$asset")
        [ -n "$got" ] || fail "no SHA-256 tool (sha256sum, shasum or openssl) is installed; install one, or set WOLFPACK_SKIP_CHECKSUM=1 to install without the check"
        [ "$got" = "$want" ] || fail "the download of $asset is damaged (SHA-256 $got, expected $want); try again"
        say "checked the SHA-256 sum"
    fi

    tar -xzf "$work/$asset" -C "$work" || fail "cannot unpack $asset"
    [ -f "$work/wolf" ] || fail "$asset does not contain wolf"
    chmod 755 "$work/wolf"
    if ! "$work/wolf" --version >/dev/null 2>&1; then
        fail "the downloaded wolf does not run on this system ($(uname -s) $(uname -m)); if $work is on a noexec file system, set TMPDIR to another directory"
    fi

    mkdir -p "$dir" || fail "cannot create $dir; choose another directory with --dir"
    [ -w "$dir" ] || fail "$dir is not writable; choose another directory with --dir"
    # A copy next to the target and a rename: a running wolf is never half overwritten.
    cp "$work/wolf" "$dir/.wolf.new.$$" || fail "cannot write into $dir"
    mv -f "$dir/.wolf.new.$$" "$dir/wolf" || fail "cannot replace $dir/wolf"
    say "installed $("$dir/wolf" --version) into $dir"

    if [ -n "$modify_path" ]; then
        add_to_path "$dir"
    fi
    other=$(command -v wolf 2>/dev/null || true)
    if [ -n "$other" ] && [ "$other" != "$dir/wolf" ]; then
        say "note: another wolf is earlier on PATH: $other"
    fi

    if [ -n "$run_setup" ]; then
        say "installing the tools of the targets with \`wolf setup\` (about 1 GB, into ~/.wolfpack)"
        "$dir/wolf" setup || say "some tools could not be installed; run \`wolf setup\` again later"
    fi

    echo
    say "done. Open a new terminal, then try:"
    echo "    wolf new hello && cd hello && wolf run"
}

main "$@"
