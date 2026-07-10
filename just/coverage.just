# Coverage recipes.
#
# `just test` (see check.just) already gates Rust unit/integration coverage.
# These recipes add the reportage e2e run: on its own, and merged with the
# Rust tests so a single number covers both the library and the CLI process
# (including src/main.rs, which the unit tests cannot reach).
#
# One instrumented build shared by both runs: merging .profraw from separate
# builds would mismatch coverage maps. cargo llvm-cov owns target/llvm-cov-target,
# so both the test run and the e2e run must feed .profraw into that directory.

# The cargo bin target these recipes report on. Must match bin/shim's
# `entry_point`; the shim builds `<target-dir>/debug/{{bin}}`.
bin := "enozunu"
cov_dir := `echo "$(pwd)/target/llvm-cov-target"`

# report merged coverage from the Rust tests and the reportage e2e run, and write cov.json
[group('coverage')]
coverage: coverage-clean coverage-setup-binary coverage-rust coverage-e2e
  just coverage-report cov.json

alias test := coverage

[private]
coverage-e2e:
  #!/usr/bin/env sh
  set -eu

  ENOZUNU_COV_BIN="{{cov_dir}}/debug/{{bin}}" ENOZUNU_COV_PROFRAW="{{cov_dir}}" reportage

[private]
coverage-rust:
  cargo llvm-cov nextest --locked --all-features --no-report

[private]
coverage-report dist_file:
  # exclude coverage of test codes.
  cargo llvm-cov report --codecov --output-path cov.json
  cargo llvm-cov report \
    --fail-under-functions 80 \
    --fail-under-lines 80 \
    --fail-under-file-lines 80 \
    --fail-under-regions 80 \
    --ignore-filename-regex "tests"

[private]
coverage-clean:
  cargo llvm-cov clean

[private]
coverage-setup-binary:
  # Ensure the CLI binary itself is built and instrumented in the same directory.
  # This is necessary because cargo-llvm-cov not provide single `build` sub command.
  # This task is dummy to materialize binary.
  cargo llvm-cov run --locked --no-report --bin {{bin}} -- --version >/dev/null
