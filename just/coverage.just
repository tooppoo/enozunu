# Coverage recipes.
#
# `just test` (see check.just) already gates Rust unit/integration coverage.
# These recipes add the reportage e2e run: on its own, and merged with the
# Rust tests so a single number covers both the library and the CLI process
# (including src/main.rs, which the unit tests cannot reach).

# The cargo bin target these recipes report on. Must match bin/shim's
# `entry_point`; the shim builds `<target-dir>/debug/{{bin}}`.
bin := "enozunu"

# report coverage from the reportage e2e run alone
[group('coverage')]
coverage-e2e:
  #!/usr/bin/env sh
  set -eu
  # Same cargo llvm-cov flow as coverage-merged, minus the test run: instrument
  # a build, drive the e2e suite against it in merge mode, then report.
  cargo llvm-cov clean --workspace
  cargo llvm-cov run --locked --no-report --bin {{bin}} -- --version >/dev/null
  cov_dir="$(pwd)/target/llvm-cov-target"
  ENOZUNU_COV_BIN="$cov_dir/debug/{{bin}}" ENOZUNU_COV_PROFRAW="$cov_dir" reportage
  cargo llvm-cov report --ignore-filename-regex 'tests/'

# report merged coverage from the Rust tests and the reportage e2e run, and write cov.json
[group('coverage')]
coverage-merged:
  #!/usr/bin/env sh
  set -eu
  # One instrumented build shared by both runs: merging .profraw from separate
  # builds would mismatch coverage maps. cargo llvm-cov owns target/llvm-cov-target,
  # so both the test run and the e2e run must feed .profraw into that directory.
  cargo llvm-cov clean --workspace
  cargo llvm-cov nextest --locked --all-features --workspace --no-report
  # Ensure the CLI binary itself is built and instrumented in the same directory.
  cargo llvm-cov run --locked --no-report --bin {{bin}} -- --version >/dev/null
  cov_dir="$(pwd)/target/llvm-cov-target"
  # ENOZUNU_COV_* switch bin/shim into merge mode: run this binary, collect here.
  ENOZUNU_COV_BIN="$cov_dir/debug/{{bin}}" ENOZUNU_COV_PROFRAW="$cov_dir" reportage
  cargo llvm-cov report --codecov --output-path cov.json
  cargo llvm-cov report --ignore-filename-regex 'tests/'
