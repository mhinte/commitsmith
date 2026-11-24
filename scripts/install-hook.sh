#!/bin/sh
# Thin wrapper: prefer `commit-tui hooks install`.
set -eu
exec commit-tui hooks install "$@"
