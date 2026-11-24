#!/bin/sh
# Thin wrapper: prefer `coggit hooks install`.
set -eu
exec coggit hooks install "$@"
