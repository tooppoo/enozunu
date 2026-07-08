
# run all checks
[group('check')]
check: test fmt lint

# run all tests and generate coverage report
[group('check')]
test:
  #!/usr/bin/env sh
  set -eu

  cargo llvm-cov --locked --all-features --workspace --no-report nextest
  cargo llvm-cov report --codecov --output-path cov.json
  cargo llvm-cov report \
    --fail-under-functions 80 \
    --fail-under-lines 80 \
    --fail-under-file-lines 80 \
    --fail-under-regions 80 \
    --ignore-filename-regex "src/main"

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
