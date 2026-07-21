
#!/usr/bin/env sh
set -eu

script_dir="$(CDPATH= cd -- "$(dirname -- "$0")" && pwd -P)"
repo_root="$(CDPATH= cd -- "$script_dir/.." && pwd -P)"
cargo_toml="$repo_root/Cargo.toml"

if [ ! -f "$cargo_toml" ]; then
  echo "Cargo.toml not found: $cargo_toml" >&2
  exit 1
fi

grep -e "^version" "$cargo_toml" \
  | cut -d"=" -f2 \
  | sed "s/ //g" \
  | sed 's/"//g'
