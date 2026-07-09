
#!/usr/bin/env sh
set -eu

script_dir="$(CDPATH= cd -- "$(dirname -- "$0")" && pwd -P)"
repo_root="$(CDPATH= cd -- "$script_dir/.." && pwd -P)"
cargo_toml="$repo_root/Cargo.toml"

if [ ! -f "$cargo_toml" ]; then
  echo "Cargo.toml not found: $cargo_toml" >&2
  exit 1
fi

version="$(
  awk '
    BEGIN {
      in_workspace_package = 0
    }

    /^[[:space:]]*\[/ {
      in_workspace_package = ($0 ~ /^[[:space:]]*\[workspace\.package\][[:space:]]*$/)
      next
    }

    in_workspace_package && /^[[:space:]]*version[[:space:]]*=/ {
      line = $0

      sub(/^[[:space:]]*version[[:space:]]*=[[:space:]]*/, "", line)
      sub(/[[:space:]]*#.*$/, "", line)
      sub(/^[[:space:]]*"/, "", line)
      sub(/"[[:space:]]*$/, "", line)

      print line
      found = 1
      exit
    }

    END {
      if (!found) {
        exit 1
      }
    }
  ' "$cargo_toml"
)" || {
  echo "workspace.package.version not found in $cargo_toml" >&2
  exit 1
}

case "$version" in
  "")
    echo "workspace.package.version is empty in $cargo_toml" >&2
    exit 1
    ;;
esac

printf '%s\n' "$version"
