import "just/check.just"
import "just/coverage.just"
import "just/release.just"

# build the project in development mode
[group('build')]
build-dev:
  cargo build --locked

# build the project in release mode
[group('build')]
[group('release')]
build-release:
  cargo build --release --locked

[group('ai')]
ai-setup: build-release
  enozunu summon
  git kura tools install --all

# regenerate examples/README.md from the example scenarios
[group('docs')]
examples-gen:
  reportage docs 'examples/**/*.repor' \
    --out-dir examples \
    --index-file-name README.md \
    --format markdown \
    --title "Enozunu Examples"

# check that examples/README.md is up to date with the example scenarios
[group('docs')]
[group('check')]
examples-check:
  #!/usr/bin/env sh
  set -eu
  tmp=$(mktemp -d)
  trap 'rm -rf "$tmp"' EXIT
  reportage docs 'examples/**/*.repor' \
    --out-dir "$tmp" \
    --index-file-name README.md \
    --format markdown \
    --title "Enozunu Examples" > /dev/null
  if ! diff -q examples/README.md "$tmp/README.md" > /dev/null 2>&1; then
    echo "examples/README.md is stale. Run 'just examples-gen' to regenerate."
    diff examples/README.md "$tmp/README.md" || true
    exit 1
  fi
  echo "examples/README.md is up to date."
