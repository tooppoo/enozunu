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
