#!/usr/bin/sh

set -eu

cargo install cargo-binstall@1.20.1 --locked
cargo binstall cargo-llvm-cov@0.8.7 \
  just@1.54.0 \
  cargo-nextest@0.9.138 \
  toml-cli@0.2.3 \
  -y \
  --locked
