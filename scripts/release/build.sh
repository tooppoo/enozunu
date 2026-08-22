#!/usr/bin/env sh

set -eu

target=${1%/}
dist=${2%/}

cargo build \
  --release \
  --locked \
  --quiet \
  --target "$target"

case "$target" in
  *-windows-*) bin=enozunu.exe ;;
  *) bin=enozunu ;;
esac

mkdir -p "${dist}"
cp "target/${target}/release/${bin}" "${dist}/${bin}"
