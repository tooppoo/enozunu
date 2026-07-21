
[group('release')]
release-tag version:
  #!/usr/bin/env sh
  rellog ready {{ version }}

  current="$(sh scripts/get-version.sh)"

  if [ "$current" != "{{ version }}" ]; then
    echo "Current version ($current) does not match the release version ({{ version }})"
    exit 1
  fi

  git tag -a {{ version }} -m "Release {{ version }}"

[group('release')]
get-version:
  @sh scripts/get-version.sh

[group('release')]
update-version version:
  rellog ready {{ version }}

  toml set Cargo.toml package.version {{ version }} > Cargo.toml.tmp
  mv Cargo.toml.tmp Cargo.toml
