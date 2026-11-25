#!/bin/sh
# Build a release here instead of on GitHub's metered runners.
#
#   scripts/release-local.sh v0.11.1            # build + package into dist/, no upload
#   scripts/release-local.sh v0.11.1 --publish  # also create the release and upload
#
# Only targets that this machine can actually link are built; the script says
# which ones it skipped and why. Everything it produces matches the names the
# installers look for: commitsmith-<version>-<target>.tar.gz plus SHA256SUMS.
set -eu

tag=""
publish=0
for arg in "$@"; do
  case "$arg" in
    --publish) publish=1 ;;
    -*) echo "unknown option $arg" >&2; exit 2 ;;
    *) tag="$arg" ;;
  esac
done

if [ -z "$tag" ]; then
  echo "usage: $0 v<version> [--publish]" >&2
  exit 2
fi

version="${tag#v}"
root="$(cd "$(dirname "$0")/.." && pwd)"
cd "$root"

echo "== version"
cargo_version="$(sed -n 's/^version = "\(.*\)"/\1/p' Cargo.toml | head -1)"
if [ "$cargo_version" != "$version" ]; then
  echo "Cargo.toml says $cargo_version, the tag says $version — bump one of them" >&2
  exit 1
fi

echo "== checks"
cargo fmt --all -- --check
cargo clippy --all-targets -- -D warnings
cargo test --all-targets --quiet

echo "== build"
mkdir -p dist/upload
targets=""
if rustup target list --installed | grep -qx x86_64-unknown-linux-gnu; then
  targets="$targets x86_64-unknown-linux-gnu"
fi
if command -v musl-gcc >/dev/null 2>&1 && rustup target list --installed | grep -qx x86_64-unknown-linux-musl; then
  targets="$targets x86_64-unknown-linux-musl"
else
  echo "   skipping x86_64-unknown-linux-musl (no musl-gcc here)"
fi
if [ -z "$targets" ]; then
  echo "no buildable target on this machine" >&2
  exit 1
fi

for target in $targets; do
  echo "   $target"
  cargo build --release --locked --target "$target"
  archive="commitsmith-${version}-${target}"
  pkg="dist/${archive}"
  rm -rf "$pkg"
  mkdir -p "$pkg"
  cp "target/${target}/release/commitsmith" "$pkg/commitsmith"
  cp README.md LICENSE "$pkg/"
  chmod +x "$pkg/commitsmith"
  # Deterministic archive: same content, same bytes.
  tar --sort=name --owner=0 --group=0 --numeric-owner \
      --mtime="@$(date -u -r README.md +%s)" \
      -C dist -czf "dist/upload/${archive}.tar.gz" "${archive}"
  (cd dist/upload && sha256sum "${archive}.tar.gz" > "${archive}.tar.gz.sha256")
done

echo "== checksums"
(cd dist/upload && cat ./*.sha256 | sort > SHA256SUMS && sha256sum -c SHA256SUMS)

if [ "$publish" -eq 0 ]; then
  echo
  echo "packaged into dist/upload; nothing uploaded (pass --publish to release $tag)"
  ls -l dist/upload
  exit 0
fi

echo "== publish $tag"
: "${GITHUB_TOKEN:?GITHUB_TOKEN is not set}"
repo="mhinte/commitsmith"
body="$(sed -n "/^## ${version}\$/,/^## /p" CHANGELOG.md 2>/dev/null | sed '1d;$d')"
[ -n "$body" ] || body="Release ${version}, built locally."

# Create (or reuse) the release, then upload every artefact.
existing="$(curl -sS -H "Authorization: token $GITHUB_TOKEN" \
  "https://api.github.com/repos/${repo}/releases/tags/${tag}" |
  sed -n 's/.*"id": \([0-9]*\).*/\1/p' | head -1)"

if [ -z "$existing" ]; then
  existing="$(curl -sS -X POST -H "Authorization: token $GITHUB_TOKEN" \
    -H "Accept: application/vnd.github+json" \
    "https://api.github.com/repos/${repo}/releases" \
    -d "$(printf '{"tag_name":"%s","name":"%s","body":%s}' "$tag" "$tag" \
          "$(printf '%s' "$body" | /usr/bin/python3 -c 'import json,sys;print(json.dumps(sys.stdin.read()))')")" |
    sed -n 's/.*"id": \([0-9]*\).*/\1/p' | head -1)"
fi

if [ -z "$existing" ]; then
  echo "could not create or find the release $tag" >&2
  exit 1
fi

for file in dist/upload/*.tar.gz dist/upload/SHA256SUMS; do
  name="$(basename "$file")"
  echo "   $name"
  curl -sS -X POST -H "Authorization: token $GITHUB_TOKEN" \
    -H "Accept: application/vnd.github+json" \
    -H "Content-Type: application/octet-stream" \
    "https://uploads.github.com/repos/${repo}/releases/${existing}/assets?name=${name}" \
    --data-binary "@${file}" > /dev/null
done

echo "published https://github.com/${repo}/releases/tag/${tag}"
