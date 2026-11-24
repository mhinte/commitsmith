#!/bin/sh
# Install commitsmith for the current user and make it reachable everywhere.
#
#   curl -sSL https://raw.githubusercontent.com/mhinte/commitsmith/main/scripts/install.sh | sh
#   ./scripts/install.sh --binary ./target/release/commitsmith --alias smith
#
# Installs to ~/.local/bin (or $COGGIT_INSTALL_DIR), writes an alias as a small
# wrapper script and tells you if the directory is missing from PATH.
set -eu

repo="mhinte/commitsmith"
install_dir="${COGGIT_INSTALL_DIR:-$HOME/.local/bin}"
binary=""
alias_name=""
tag=""

while [ $# -gt 0 ]; do
    case "$1" in
        --binary) binary="$2"; shift 2 ;;
        --alias) alias_name="$2"; shift 2 ;;
        --tag) tag="$2"; shift 2 ;;
        --dir) install_dir="$2"; shift 2 ;;
        -h|--help)
            sed -n '2,9p' "$0" | sed 's/^# \{0,1\}//'
            exit 0 ;;
        *) echo "unknown option: $1" >&2; exit 2 ;;
    esac
done

step() { printf '  %s\n' "$1"; }
# Works out the release asset for this machine.
asset_name() {
    case "$(uname -s)" in
        Linux) os="unknown-linux-musl" ;;
        Darwin) os="apple-darwin" ;;
        *) echo "unsupported system: $(uname -s)" >&2; return 1 ;;
    esac
    case "$(uname -m)" in
        x86_64|amd64) arch="x86_64" ;;
        aarch64|arm64) arch="aarch64" ;;
        *) echo "unsupported machine: $(uname -m)" >&2; return 1 ;;
    esac
    printf '%s-%s' "$arch" "$os"
}

if [ -n "$binary" ]; then
    [ -f "$binary" ] || { echo "binary not found: $binary" >&2; exit 1; }
    step "using $binary"
    source_bin="$binary"
else
    target=$(asset_name)
    if [ -n "$tag" ]; then
        url="https://api.github.com/repos/$repo/releases/tags/$tag"
    else
        url="https://api.github.com/repos/$repo/releases/latest"
    fi
    step "asking $url"
    release_json=$(curl -fsSL -H 'Accept: application/vnd.github+json' "$url")
    version=$(printf '%s' "$release_json" \
        | sed -n 's/.*"tag_name": *"\([^"]*\)".*/\1/p' | head -1)
    [ -n "$version" ] || { echo "cannot read the release tag" >&2; exit 1; }
    file="commitsmith-${version#v}-${target}.tar.gz"
    step "downloading $file"
    tmp=$(mktemp -d)
    trap 'rm -rf "$tmp"' EXIT INT TERM
    curl -fsSL -o "$tmp/$file" \
        "https://github.com/$repo/releases/download/$version/$file" || {
            echo "no archive named $file in $version" >&2; exit 1; }
    tar -xzf "$tmp/$file" -C "$tmp"
    source_bin=$(find "$tmp" -type f -name commitsmith | head -1)
    [ -n "$source_bin" ] || { echo "the archive did not contain commitsmith" >&2; exit 1; }
fi

mkdir -p "$install_dir"
cp "$source_bin" "$install_dir/commitsmith"
chmod 755 "$install_dir/commitsmith"
step "installed $install_dir/commitsmith"

if [ -n "$alias_name" ]; then
    # A tiny wrapper, so the alias keeps following upgrades of the real binary.
    cat > "$install_dir/$alias_name" <<EOF
#!/bin/sh
exec "$install_dir/commitsmith" "\$@"
EOF
    chmod 755 "$install_dir/$alias_name"
    step "alias $install_dir/$alias_name"
fi

case ":$PATH:" in
    *":$install_dir:"*) step "$install_dir is on your PATH" ;;
    *)
        step "$install_dir is NOT on your PATH"
        printf '  add this to your shell profile:\n    export PATH="%s:$PATH"\n' "$install_dir"
        ;;
esac

"$install_dir/commitsmith" --version
