#!/usr/bin/env sh
set -eu

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"

for script in "$SCRIPT_DIR/dev/"*.sh; do
  [ -f "$script" ] || continue
  echo "Running $(basename "$script")..."
  sh "$script"
done
