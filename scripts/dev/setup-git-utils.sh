#!/usr/bin/env sh

set -eu

curl -fsSL https://raw.githubusercontent.com/tooppoo/git-utils/main/install.sh \
  | sh -s -- git-commits-since-tag git-rm-branch
