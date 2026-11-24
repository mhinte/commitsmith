#!/bin/sh
# Install a commit-msg hook that runs `commit-tui check`.
set -eu

script_dir=$(CDPATH= cd -- "$(dirname -- "$0")" && pwd)

git rev-parse --show-toplevel >/dev/null 2>&1 || {
    echo "commit-tui: not inside a git repository" >&2
    exit 1
}

hook_path="$(git rev-parse --git-path hooks)/commit-msg"
mkdir -p "$(dirname "$hook_path")"
cp "$script_dir/commit-msg" "$hook_path"
chmod +x "$hook_path"

echo "installed $hook_path"

command -v commit-tui >/dev/null 2>&1 || \
    echo "warning: commit-tui is not on PATH, install it with 'cargo install --path .'" >&2
