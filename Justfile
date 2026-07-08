import "just/check.just"

# build the project in development mode
[group('build')]
build-dev:
  cargo build --locked

# build the project in release mode
[group('build')]
[group('release')]
build-release:
  cargo build --release --locked
