#!/usr/bin/env sh
set -eu

if ! git rev-parse --is-inside-work-tree >/dev/null 2>&1; then
  echo "Not inside a Git work tree. Skipping repository-local Git config."
  exit 0
fi

git config --local core.autocrlf false
git config --local core.eol lf
git config --local core.safecrlf warn

# Devcontainer/Linux semantics.
# This does not make a case-insensitive host filesystem case-sensitive.
git config --local core.ignorecase false

# Prefer meaningful executable-bit tracking inside Linux/devcontainer.
git config --local core.filemode true

git config --local pull.ff only
git config --local fetch.prune true
git config --local diff.renames true

echo "Repository-local Git config has been applied."
