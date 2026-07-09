#!/usr/bin/env sh

set -eu

target=${1%/}
dist=${2%/}

cargo build \
  --release \
  --locked \
  --quiet \
  --target "$target"

mkdir -p "${dist}"
cp "target/${target}/release/enozunu" "${dist}/enozunu"
