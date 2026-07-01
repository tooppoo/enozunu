#!/usr/bin/env sh
set -eu

REPO_ROOT="$(cd "$(dirname "$0")/../.." && pwd)"
BIN_DIR="$REPO_ROOT/bin"

if ! grep -qF "$BIN_DIR" "$HOME/.bashrc" 2>/dev/null; then
  printf '\nexport PATH="%s:$PATH"\n' "$BIN_DIR" >> "$HOME/.bashrc"
  echo "Added $BIN_DIR to PATH in ~/.bashrc"
else
  echo "$BIN_DIR is already configured in ~/.bashrc"
fi
