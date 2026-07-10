import "coverage.just"

# run all checks
[group('check')]
check: coverage fmt lint

# run all formatting checks
[group('check')]
fmt:
  cargo fmt --all --check

# fix all formatting issues
[group('check')]
fmt-fix:
  cargo fmt --all

# run all lint checks
[group('check')]
lint:
  cargo clippy --locked -- -D warnings
