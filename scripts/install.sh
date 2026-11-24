#!/bin/sh
# Install commitsmith for the current user and make it reachable everywhere.
#
#   curl -sSL https://raw.githubusercontent.com/mhinte/commitsmith/main/scripts/install.sh | sh
#   ./scripts/install.sh --self-test v0.3.0     # check an archive without installing
#   ./scripts/install.sh --binary ./target/release/commitsmith --alias smith
#
# Downloads the release archive for this machine, verifies it against the
# published SHA256SUMS and hands over to `commitsmith --install`, which puts the
# binary in a user directory and fixes PATH.
set -eu

repo="mhinte/commitsmith"
alias_name=""
self_tag=""
tag=""
status=0
# Release host, overridable so the installer can be tested offline.
url_base="${COMMITSMITH_URL_BASE:-https://github.com/$repo/releases/download}"

while [ $# -gt 0 ]; do
    case "$1" in
        --alias) alias_name="$2"; shift 2 ;;
        --tag) tag="$2"; shift 2 ;;
        --self-test) self_tag="$2"; shift 2 ;;
        --url-base) url_base="$2"; shift 2 ;;
        -h|--help)
            sed -n '2,10p' "$0" | sed 's/^# \{0,1\}//'
            exit 0 ;;
        *) echo "unknown option: $1" >&2; exit 2 ;;
    esac
done

# Diagnostics go to stderr so that command substitution captures data only.
step() { printf '  %s\n' "$1" >&2; }

# Works out the release asset for this machine; the static musl build is used on
# Linux so one archive runs everywhere.
asset_name() {
    case "$(uname -s)" in
        Linux) os="unknown-linux-musl" ;;
        Darwin) os="apple-darwin" ;;
        *) echo "unsupported system: $(uname -s)" >&2; status=1; return 1 ;;
    esac
    case "$(uname -m)" in
        x86_64|amd64) arch="x86_64" ;;
        aarch64|arm64) arch="aarch64" ;;
        *) echo "unsupported machine: $(uname -m)" >&2; status=1; return 1 ;;
    esac
    printf '%s-%s' "$arch" "$os"
}

download_and_verify() {
    version="$1"
    target=$(asset_name) || return 1
    file="commitsmith-${version#v}-${target}.tar.gz"
    tmp=$(mktemp -d)
    base="$url_base/$version"

    step "downloading $file"
    curl -fsSL -o "$tmp/$file" "$base/$file" ||
        { echo "no archive named $file in $version" >&2; status=1; return 1; }

    if curl -fsSL -o "$tmp/SHA256SUMS" "$base/SHA256SUMS"; then
        # Only the line for our file, then verify inside the temp dir.
        ( cd "$tmp" && grep " $file\$" SHA256SUMS > check && sha256sum -c check >/dev/null ) ||
            { echo "checksum mismatch for $file" >&2; status=1; return 1; }
        step "checksum ok"
    else
        step "no SHA256SUMS in this release, skipping verification"
    fi

    tar -xzf "$tmp/$file" -C "$tmp"
    # Wait for the extracted file to be a file: on a fresh extraction the binary
    # must not be cached as "missing".
    [ -x "$tmp/commitsmith-${version#v}-${target}/commitsmith" ] || {
        echo "the archive did not contain the binary" >&2
        status=1
        return 1
    }
    printf '%s\n' "$tmp/commitsmith-${version#v}-${target}/commitsmith"
}

# --self-test: prove that a release archive unpacks and runs, then stop.
if [ -n "$self_tag" ]; then
    bin=$(download_and_verify "$self_tag") || exit 1
    "$bin" --version
    rm -rf "$(dirname "$(dirname "$bin")")"
    exit 0
fi

if [ -n "$tag" ]; then
    url="https://api.github.com/repos/$repo/releases/tags/$tag"
else
    url="https://api.github.com/repos/$repo/releases/latest"
fi
step "asking $url"
release_json=$(curl -fsSL -H 'Accept: application/vnd.github+json' "$url") ||
    { echo "cannot read the release list" >&2; exit 1; }
version=$(printf '%s' "$release_json" \
    | sed -n 's/.*"tag_name": *"\([^"]*\)".*/\1/p' | head -1)
[ -n "$version" ] || { echo "cannot read the release tag" >&2; exit 1; }

bin=$(download_and_verify "$version") || exit 1
tmp_root=$(dirname "$(dirname "$bin")")

# The binary knows where it belongs; it also handles PATH and the alias.
if [ -n "$alias_name" ]; then
    set -- --install --alias "$alias_name"
else
    set -- --install
fi
"$bin" "$@"
rm -rf "$tmp_root"

step "run: commitsmith --version in a new shell"
