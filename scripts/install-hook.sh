#!/bin/sh
# Thin wrapper: prefer `commitsmith hooks install`.
set -eu
exec commitsmith hooks install "$@"
