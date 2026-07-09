
#!/bin/sh
# Review this script before running it.
# This generated installer is provided as-is, without any warranty.
# The target repository, release assets, and installed software remain subject to their own licenses.
set -u

if [ "${DEBUG:-}" = "1" ]; then
  set -x
fi

#
# Effective installer configuration:
#   generator.name: installerer
#   generator.sourceUrl: https://github.com/tooppoo/installerer
#   owner: tooppoo
#   repo: enozunu
#   binary.name: enozunu
#   binary.pathInArchive: enozunu
#   archive.format: tar.gz
#   archive.nameTemplate: {repo}_{version}_{os}_{arch}.tar.gz
#   archive.osCase: capitalized
#   checksum.fileName: checksums.txt
#   checksum.algorithm: sha256
#   defaults.installDir: $HOME/.local/bin
#   targets: linux/x86_64, linux/aarch64, darwin/x86_64, darwin/aarch64

OWNER='tooppoo'
REPO='enozunu'
BINARY_NAME='enozunu'
BINARY_PATH_IN_ARCHIVE='enozunu'
CHECKSUM_FILE_NAME='checksums.txt'
# shellcheck disable=SC2088 # a leading '~' here is a literal default, expanded manually by resolve_install_dir, not by the shell
DEFAULT_INSTALL_DIR='$HOME/.local/bin'
INSTALL_DIR=
ARCHIVE_FORMAT='tar.gz'
ARCHIVE_SUFFIX='.tar.gz'
LF='
'
CR=$(printf '\r')

main() {
  version=
  install_dir_raw=$DEFAULT_INSTALL_DIR
  saw_version=0
  saw_install_dir=0
  saw_requirements=0
  saw_check_requirements=0

  while [ "$#" -gt 0 ]; do
    case "$1" in
      --help)
        usage
        exit 0
        ;;
      --version)
        [ "$#" -ge 2 ] || fail "--version requires a value"
        version=$2
        saw_version=1
        shift 2
        ;;
      --install-dir)
        [ "$#" -ge 2 ] || fail "--install-dir requires a value"
        install_dir_raw=$2
        saw_install_dir=1
        shift 2
        ;;
      --requirements)
        saw_requirements=1
        shift
        ;;
      --check-requirements)
        saw_check_requirements=1
        shift
        ;;
      *)
        usage >&2
        fail "unknown argument: $1"
        ;;
    esac
  done

  if [ "$saw_requirements" -eq 1 ] || [ "$saw_check_requirements" -eq 1 ]; then
    if [ "$saw_version" -eq 1 ] || [ "$saw_install_dir" -eq 1 ]; then
      fail "--requirements/--check-requirements must not be combined with --version/--install-dir"
    fi
    [ "$saw_requirements" -eq 0 ] || print_requirements
    if [ "$saw_check_requirements" -eq 1 ]; then
      check_requirements
      exit $?
    fi
    exit 0
  fi

  [ "$version" != "latest" ] || fail "--version latest is ambiguous; omit --version for latest install"
  INSTALL_DIR=$(resolve_install_dir "$install_dir_raw")
  [ -n "$INSTALL_DIR" ] || fail "install directory must not be empty"
  validate_binary_path_in_archive "$BINARY_PATH_IN_ARCHIVE"
  check_runtime_dependencies

  if [ -n "$version" ]; then
    install_pin "$version"
  else
    install_latest
  fi
}

fail() {
  printf '%s\n' "installerer: $*" >&2
  exit 1
}

usage() {
  printf '%s\n' "usage: $0 [--version <version>] [--install-dir <dir>]"
  printf '%s\n' "       $0 --requirements [--check-requirements]"
  printf '%s\n' "       $0 --check-requirements"
  printf '%s\n' "       $0 --help"
  printf '%s\n' ''
  printf '%s\n' 'options:'
  printf '%s\n' '  --version <version>     Install a specific release tag instead of latest.'
  printf '%s\n' '  --install-dir <dir>     Install into <dir> instead of the default install directory.'
  printf '%s\n' '  --requirements          Print the runtime requirements for this installer.'
  printf '%s\n' '  --check-requirements    Check whether runtime requirements are satisfied.'
  printf '%s\n' '  --help                  Show this help message.'
  printf '%s\n' ''
  printf '%s\n' 'local execution examples:'
  printf '%s\n' '  sh install.sh'
  printf '%s\n' '  sh install.sh --version v0.1.2'
  printf '%s\n' '  sh install.sh --install-dir "$HOME/bin"'
  printf '%s\n' '  sh install.sh --version v0.1.2 --install-dir "$HOME/bin"'
}

print_requirements() {
  printf '%s\n' 'Runtime requirements for this installer:'
  printf '%s\n' ''
  printf '%s\n' 'Runtime premise:'
  printf '%s\n' '- POSIX-compatible sh: this installer is executed under a POSIX-compatible sh runtime'
  printf '%s\n' ''
  printf '%s\n' 'Required commands:'
  printf '%s\n' '- uname: Detects the host OS and architecture.'
  printf '%s\n' '- mktemp: Creates a private temporary workspace for download and extraction.'
  printf '%s\n' '- rm: Cleans up temporary files.'
  printf '%s\n' '- mkdir: Creates the install directory and the archive extraction directory.'
  printf '%s\n' '- cp: Copies the extracted binary into the install directory.'
  printf '%s\n' '- mv: Installs the binary atomically.'
  printf '%s\n' '- chmod: Makes the installed binary executable.'
  printf '%s\n' '- curl: Downloads release files from GitHub release assets.'
  printf '%s\n' '- awk: Encodes URL path segments and looks up checksum entries.'
  printf '%s\n' '- grep: Validates archive filenames.'
  printf '%s\n' '- od: Encodes URL path segments safely.'
  printf '%s\n' '- tr: Encodes URL path segments and canonicalizes OS names.'
  printf '%s\n' '- cut: Encodes URL path segments safely.'
  printf '%s\n' '- ls: Lists downloaded and extracted files for diagnostics.'
  printf '%s\n' '- tar: Extracts tar.gz archives.'
  printf '%s\n' '- sha256sum or shasum: Verifies SHA-256 checksums.'
  printf '%s\n' ''
  printf '%s\n' 'Network:'
  printf '%s\n' '- HTTPS access to GitHub release assets: downloads the archive and checksum file from GitHub Releases'
  printf '%s\n' ''
  printf '%s\n' 'Filesystem:'
  printf '%s\n' '- Write permission to the install directory: the installer writes the binary into the install directory'
}

check_requirements() {
  status=0
  printf '%s\n' "Checking runtime requirements..."
  printf '\n'
  printf '%s\n' "Runtime premise:"
  printf '%s\n' '- POSIX-compatible sh: this installer is executed under a POSIX-compatible sh runtime'
  printf '\n'
  if command -v 'uname' >/dev/null 2>&1; then
    printf 'ok: %s\n' 'uname'
  else
    printf 'missing: %s\n' 'uname'
    status=1
  fi
  if command -v 'mktemp' >/dev/null 2>&1; then
    printf 'ok: %s\n' 'mktemp'
  else
    printf 'missing: %s\n' 'mktemp'
    status=1
  fi
  if command -v 'rm' >/dev/null 2>&1; then
    printf 'ok: %s\n' 'rm'
  else
    printf 'missing: %s\n' 'rm'
    status=1
  fi
  if command -v 'mkdir' >/dev/null 2>&1; then
    printf 'ok: %s\n' 'mkdir'
  else
    printf 'missing: %s\n' 'mkdir'
    status=1
  fi
  if command -v 'cp' >/dev/null 2>&1; then
    printf 'ok: %s\n' 'cp'
  else
    printf 'missing: %s\n' 'cp'
    status=1
  fi
  if command -v 'mv' >/dev/null 2>&1; then
    printf 'ok: %s\n' 'mv'
  else
    printf 'missing: %s\n' 'mv'
    status=1
  fi
  if command -v 'chmod' >/dev/null 2>&1; then
    printf 'ok: %s\n' 'chmod'
  else
    printf 'missing: %s\n' 'chmod'
    status=1
  fi
  if command -v 'curl' >/dev/null 2>&1; then
    printf 'ok: %s\n' 'curl'
  else
    printf 'missing: %s\n' 'curl'
    status=1
  fi
  if command -v 'awk' >/dev/null 2>&1; then
    printf 'ok: %s\n' 'awk'
  else
    printf 'missing: %s\n' 'awk'
    status=1
  fi
  if command -v 'grep' >/dev/null 2>&1; then
    printf 'ok: %s\n' 'grep'
  else
    printf 'missing: %s\n' 'grep'
    status=1
  fi
  if command -v 'od' >/dev/null 2>&1; then
    printf 'ok: %s\n' 'od'
  else
    printf 'missing: %s\n' 'od'
    status=1
  fi
  if command -v 'tr' >/dev/null 2>&1; then
    printf 'ok: %s\n' 'tr'
  else
    printf 'missing: %s\n' 'tr'
    status=1
  fi
  if command -v 'cut' >/dev/null 2>&1; then
    printf 'ok: %s\n' 'cut'
  else
    printf 'missing: %s\n' 'cut'
    status=1
  fi
  if command -v 'ls' >/dev/null 2>&1; then
    printf 'ok: %s\n' 'ls'
  else
    printf 'missing: %s\n' 'ls'
    status=1
  fi
  if command -v 'tar' >/dev/null 2>&1; then
    printf 'ok: %s\n' 'tar'
  else
    printf 'missing: %s\n' 'tar'
    status=1
  fi
  if command -v 'sha256sum' >/dev/null 2>&1 || command -v 'shasum' >/dev/null 2>&1; then
    printf 'ok: %s\n' 'sha256sum or shasum'
  else
    printf 'missing: %s\n' 'sha256sum or shasum'
    status=1
  fi
  printf '\n'
  printf '%s\n' "Not checked:"
  printf '%s\n' '- HTTPS access to GitHub release assets: downloads the archive and checksum file from GitHub Releases'
  printf '%s\n' '- Write permission to the install directory: the installer writes the binary into the install directory'
  printf '\n'
  if [ "$status" -eq 0 ]; then
    printf '%s\n' "All checkable requirements are satisfied."
  else
    printf '%s\n' "Some checkable requirements are missing."
  fi
  return "$status"
}

require_command() {
  command -v "$1" >/dev/null 2>&1 || fail "$1 is required"
}

check_runtime_dependencies() {
  require_command 'uname'
  require_command 'mktemp'
  require_command 'rm'
  require_command 'mkdir'
  require_command 'cp'
  require_command 'mv'
  require_command 'chmod'
  require_command 'curl'
  require_command 'awk'
  require_command 'grep'
  require_command 'od'
  require_command 'tr'
  require_command 'cut'
  require_command 'ls'
  case "$ARCHIVE_FORMAT" in
    tar.gz) require_command 'tar' ;;
    zip) require_command 'unzip' ;;
    *) fail "unsupported archive format: $ARCHIVE_FORMAT" ;;
  esac
  if command -v 'sha256sum' >/dev/null 2>&1; then
    CHECKSUM_COMMAND='sha256sum'
  elif command -v 'shasum' >/dev/null 2>&1; then
    CHECKSUM_COMMAND='shasum'
  else
    fail "sha256sum or shasum is required"
  fi
}

url_encode_segment() {
  value=$1
  encoded=
  hex_bytes=$(LC_ALL=C printf '%s' "$value" | od -An -tx1 -v | tr -d ' \n')

  while [ -n "$hex_bytes" ]; do
    byte=$(printf '%s' "$hex_bytes" | cut -c 1-2)
    hex_bytes=$(printf '%s' "$hex_bytes" | cut -c 3-)
    case "$byte" in
      2d) encoded="$encoded-" ;;
      2e) encoded="$encoded." ;;
      5f) encoded="$encoded"_ ;;
      7e) encoded="$encoded~" ;;
      30) encoded="$encoded"0 ;;
      31) encoded="$encoded"1 ;;
      32) encoded="$encoded"2 ;;
      33) encoded="$encoded"3 ;;
      34) encoded="$encoded"4 ;;
      35) encoded="$encoded"5 ;;
      36) encoded="$encoded"6 ;;
      37) encoded="$encoded"7 ;;
      38) encoded="$encoded"8 ;;
      39) encoded="$encoded"9 ;;
      41) encoded="$encoded"A ;;
      42) encoded="$encoded"B ;;
      43) encoded="$encoded"C ;;
      44) encoded="$encoded"D ;;
      45) encoded="$encoded"E ;;
      46) encoded="$encoded"F ;;
      47) encoded="$encoded"G ;;
      48) encoded="$encoded"H ;;
      49) encoded="$encoded"I ;;
      4a) encoded="$encoded"J ;;
      4b) encoded="$encoded"K ;;
      4c) encoded="$encoded"L ;;
      4d) encoded="$encoded"M ;;
      4e) encoded="$encoded"N ;;
      4f) encoded="$encoded"O ;;
      50) encoded="$encoded"P ;;
      51) encoded="$encoded"Q ;;
      52) encoded="$encoded"R ;;
      53) encoded="$encoded"S ;;
      54) encoded="$encoded"T ;;
      55) encoded="$encoded"U ;;
      56) encoded="$encoded"V ;;
      57) encoded="$encoded"W ;;
      58) encoded="$encoded"X ;;
      59) encoded="$encoded"Y ;;
      5a) encoded="$encoded"Z ;;
      61) encoded="$encoded"a ;;
      62) encoded="$encoded"b ;;
      63) encoded="$encoded"c ;;
      64) encoded="$encoded"d ;;
      65) encoded="$encoded"e ;;
      66) encoded="$encoded"f ;;
      67) encoded="$encoded"g ;;
      68) encoded="$encoded"h ;;
      69) encoded="$encoded"i ;;
      6a) encoded="$encoded"j ;;
      6b) encoded="$encoded"k ;;
      6c) encoded="$encoded"l ;;
      6d) encoded="$encoded"m ;;
      6e) encoded="$encoded"n ;;
      6f) encoded="$encoded"o ;;
      70) encoded="$encoded"p ;;
      71) encoded="$encoded"q ;;
      72) encoded="$encoded"r ;;
      73) encoded="$encoded"s ;;
      74) encoded="$encoded"t ;;
      75) encoded="$encoded"u ;;
      76) encoded="$encoded"v ;;
      77) encoded="$encoded"w ;;
      78) encoded="$encoded"x ;;
      79) encoded="$encoded"y ;;
      7a) encoded="$encoded"z ;;
      *) encoded="$encoded%$(printf '%s' "$byte" | tr 'abcdef' 'ABCDEF')" ;;
    esac
  done

  printf '%s' "$encoded"
}

is_valid_git_tag() {
  tag=$1
  case "$tag" in
    ""|latest|/*|*/|*.|@|*//*|*..*|*@\{*|*~*|*^*|*:*|*\?*|*\**|*\[*|*\\*) return 1 ;;
    *"$CR"*|*"$LF"*) return 1 ;;
  esac
  if LC_ALL=C printf '%s' "$tag" | grep -q '[[:cntrl:][:space:]]'; then
    return 1
  fi
  old_ifs=$IFS
  IFS=/
  set -- $tag
  IFS=$old_ifs
  for segment do
    case "$segment" in
      ""|.*|*.lock) return 1 ;;
    esac
  done
  return 0
}

# A Git tag may legitimately contain '/' (e.g. "release/v1.2.3"), but a value
# extracted from a checksum-index archive filename cannot: '/' would split it
# across path segments. installerer treats such tags as unsupported for
# {version} extraction (issue #111) even though --version pinning still
# accepts them.
is_filename_unsafe_tag() {
  value=$1
  case "$value" in
    */*|*\\*) return 0 ;;
  esac
  if LC_ALL=C printf '%s' "$value" | grep -q '[[:cntrl:][:space:]]'; then
    return 0
  fi
  return 1
}
resolve_expected_release_tag() {
  index_path=$1
  prefix=$2
  suffix=$3
  match=
  match_count=0
  while IFS= read -r line || [ -n "$line" ]; do
    set -f
    set -- $line
    set +f
    filename=$2
    [ -n "$filename" ] || continue
    case "$filename" in
      "$prefix"*"$suffix")
        if [ "$filename" != "$match" ]; then
          match_count=$((match_count + 1))
          match=$filename
        fi
        ;;
    esac
  done < "$index_path"
  [ "$match_count" -gt 0 ] \
    || fail "no release asset in $CHECKSUM_FILE_NAME matches the configured archive filename template"
  [ "$match_count" -eq 1 ] \
    || fail "ambiguous: multiple release assets in $CHECKSUM_FILE_NAME match the configured archive filename template"
  candidate=${match#"$prefix"}
  candidate=${candidate%"$suffix"}
  is_valid_git_tag "$candidate" || fail "extracted release tag is not a valid Git tag: $candidate"
  is_filename_unsafe_tag "$candidate" && fail "extracted release tag is not safe as a filename: $candidate"
  printf '%s' "$candidate"
}

validate_archive_asset_name() {
  name=$1
  [ -n "$name" ] || fail "archive filename is empty"
  case "$name" in
    */*|*\\*) fail "archive filename contains a path separator: $name" ;;
  esac
  if LC_ALL=C printf '%s' "$name" | grep '[[:cntrl:][:space:]]' >/dev/null; then
    fail "archive filename contains whitespace or control characters: $name"
  fi
  case "$name" in
    *"$ARCHIVE_SUFFIX") ;;
    *) fail "archive filename does not end with $ARCHIVE_SUFFIX: $name" ;;
  esac
}

validate_binary_path_in_archive() {
  path=$1
  [ -n "$path" ] || fail "binary.pathInArchive is empty"
  case "$path" in
    /*|*/|*\\*) fail "binary.pathInArchive must be a relative file path: $path" ;;
  esac
  old_ifs=$IFS
  IFS=/
  set -- $path
  IFS=$old_ifs
  for segment do
    case "$segment" in
      ""|.|..) fail "binary.pathInArchive contains an unsafe path segment: $path" ;;
    esac
  done
}

resolve_install_dir() {
  raw=$1
  # shellcheck disable=SC2088 # '$HOME'/'~' below are literal glob prefixes matched against $raw, then expanded manually via $HOME; the shell never expands them itself
  case "$raw" in
    '$HOME') printf '%s' "$HOME" ;;
    '$HOME/'*) printf '%s/%s' "$HOME" "${raw#\$HOME/}" ;;
    '~') printf '%s' "$HOME" ;;
    '~/'*) printf '%s/%s' "$HOME" "${raw#\~/}" ;;
    /*) printf '%s' "$raw" ;;
    *) printf '%s' "$raw" ;;
  esac
}

detect_target() {
  os=$(uname -s | tr '[:upper:]' '[:lower:]')
  arch=$(uname -m)

  case "$os" in
    linux) os=linux ;;
    darwin) os=darwin ;;
    *) fail "unsupported OS: $os" ;;
  esac

  case "$arch" in
    x86_64) arch=x86_64 ;;
    aarch64|arm64) arch=aarch64 ;;
    *) fail "unsupported architecture: $arch" ;;
  esac

  case "$os/$arch" in
    linux/x86_64) ;;
    linux/aarch64) ;;
    darwin/x86_64) ;;
    darwin/aarch64) ;;
    *) fail "unsupported target: $os/$arch" ;;
  esac

  printf '%s %s\n' "$os" "$arch"
}

resolve_asset_arch_label() {
  canonical_os=$1
  canonical_arch=$2

  case "$canonical_os/$canonical_arch" in
    linux/x86_64) asset_arch_label='x86_64' ;;
    linux/aarch64) asset_arch_label='aarch64' ;;
    darwin/x86_64) asset_arch_label='x86_64' ;;
    darwin/aarch64) asset_arch_label='arm64' ;;
    *) fail "unsupported target: $canonical_os/$canonical_arch" ;;
  esac

  printf '%s\n' "$asset_arch_label"
}

render_archive_asset_name() {
  version=$1
  os=$2
  asset_arch_label=$3
  case "$os" in
    linux) os=Linux ;;
    darwin) os=Darwin ;;
  esac
  target="${os}_${asset_arch_label}"
  printf '%s' "$REPO" '_' "$version" '_' "$os" '_' "$asset_arch_label" '.tar.gz'
  printf '\n'
}

render_archive_asset_name_prefix() {
  os=$1
  asset_arch_label=$2
  case "$os" in
    linux) os=Linux ;;
    darwin) os=Darwin ;;
  esac
  target="${os}_${asset_arch_label}"
  printf '%s' "$REPO" '_'
  printf '\n'
}

render_archive_asset_name_suffix() {
  os=$1
  asset_arch_label=$2
  case "$os" in
    linux) os=Linux ;;
    darwin) os=Darwin ;;
  esac
  target="${os}_${asset_arch_label}"
  printf '%s' '_' "$os" '_' "$asset_arch_label" '.tar.gz'
  printf '\n'
}

curl_download() {
  url=$1
  output_path=$2
  label=$3
  printf '%s\n' "installerer: requesting $url"
  curl -fsSL "$url" -o "$output_path" || fail "failed to download $label"
  printf '%s\n' "installerer: downloaded files:"
  ls -la "$tmpdir"
}

verify_sha256() {
  expected_checksum=$(awk -v name="$archive_asset_name" '$2 == name { print $1; found=1; exit } END { if (!found) exit 1 }' "$checksum_path") \
    || fail "checksum entry not found for $archive_asset_name"
  case "$CHECKSUM_COMMAND" in
    sha256sum)
      printf '%s  %s\n' "$expected_checksum" "$archive_path" | sha256sum -c - >/dev/null \
        || fail "archive checksum mismatch"
      ;;
    shasum)
      actual_checksum=$(shasum -a 256 "$archive_path" | awk '{ print $1 }') \
        || fail "failed to compute archive checksum"
      [ "$actual_checksum" = "$expected_checksum" ] || fail "archive checksum mismatch"
      ;;
    *)
      fail "checksum command was not initialized"
      ;;
  esac
}

extract_archive() {
  mkdir -p "$extract_dir" || fail "failed to create extract directory"
  case "$ARCHIVE_FORMAT" in
    tar.gz)
      tar -xzf "$archive_path" -C "$extract_dir" -- "$BINARY_PATH_IN_ARCHIVE" \
        || fail "failed to extract $BINARY_PATH_IN_ARCHIVE from tar.gz archive"
      ;;
    zip)
      unzip -q "$archive_path" "$BINARY_PATH_IN_ARCHIVE" -d "$extract_dir" \
        || fail "failed to extract $BINARY_PATH_IN_ARCHIVE from zip archive"
      ;;
    *)
      fail "unsupported archive format: $ARCHIVE_FORMAT"
      ;;
  esac
  printf '%s\n' "installerer: extracted files:"
  ls -laR "$extract_dir"

  extracted_binary="$extract_dir/$BINARY_PATH_IN_ARCHIVE"
  [ ! -L "$extracted_binary" ] || fail "archive binary entry must not be a symlink: $BINARY_PATH_IN_ARCHIVE"
  [ -f "$extracted_binary" ] || fail "archive binary entry is not a regular file: $BINARY_PATH_IN_ARCHIVE"
}

cleanup() {
  if [ -n "${install_tmp:-}" ]; then
    rm -f "$install_tmp"
  fi
  if [ -n "${tmpdir:-}" ]; then
    rm -rf "$tmpdir"
  fi
}

cleanup_on_signal() {
  cleanup
  exit 1
}

install_binary() {
  mkdir -p -- "$INSTALL_DIR" || fail "failed to create install directory: $INSTALL_DIR"

  install_tmp=$(mktemp -- "$INSTALL_DIR/.$BINARY_NAME.tmp.XXXXXX") \
    || fail "failed to create temporary install file in $INSTALL_DIR"

  cp -- "$extracted_binary" "$install_tmp" \
    || fail "failed to copy binary to temporary install path"

  # -- must precede 755: BSD chmod (macOS) stops option parsing at the first
  # operand, so "chmod 755 -- ..." treats -- as a filename, not a terminator.
  chmod -- 755 "$install_tmp" \
    || fail "failed to set binary mode"

  mv -- "$install_tmp" "$INSTALL_DIR/$BINARY_NAME" \
    || fail "failed to place binary in install directory"

  install_tmp=
}

download_and_install() {
  archive_url=$1
  checksum_url=$2
  archive_asset_name=$3
  curl_download "$checksum_url" "$checksum_path" "checksum file"
  curl_download "$archive_url" "$archive_path" "archive"
  verify_sha256
  extract_archive
  install_binary
  printf '%s\n' "installed $BINARY_NAME to $INSTALL_DIR/$BINARY_NAME"
}

install_latest() {
  target=$(detect_target) || exit 1
  set -- $target
  os=$1
  arch=$2
  asset_arch_label=$(resolve_asset_arch_label "$os" "$arch") || exit 1
  trap cleanup EXIT
  trap cleanup_on_signal HUP INT TERM
  tmpdir=$(mktemp -d) || fail "failed to create temporary directory"
  archive_path="$tmpdir/archive"
  checksum_path="$tmpdir/checksums"
  checksum_index_path="$tmpdir/checksums_index"
  extract_dir="$tmpdir/extract"
  owner_path=$(url_encode_segment "$OWNER")
  repo_path=$(url_encode_segment "$REPO")
  checksum_index_path_segment=$(url_encode_segment "$CHECKSUM_FILE_NAME")
  checksum_index_url="https://github.com/$owner_path/$repo_path/releases/latest/download/$checksum_index_path_segment"
  curl_download "$checksum_index_url" "$checksum_index_path" "checksum index"
  prefix=$(render_archive_asset_name_prefix "$os" "$asset_arch_label")
  suffix=$(render_archive_asset_name_suffix "$os" "$asset_arch_label")
  resolved_version=$(resolve_expected_release_tag "$checksum_index_path" "$prefix" "$suffix") || exit 1
  printf '%s\n' "installerer: resolved latest version $resolved_version"
  archive_asset_name=$(render_archive_asset_name "$resolved_version" "$os" "$asset_arch_label")
  validate_archive_asset_name "$archive_asset_name"
  version_path=$(url_encode_segment "$resolved_version")
  archive_path_segment=$(url_encode_segment "$archive_asset_name")
  checksum_path_segment=$(url_encode_segment "$CHECKSUM_FILE_NAME")
  archive_url="https://github.com/$owner_path/$repo_path/releases/download/$version_path/$archive_path_segment"
  checksum_url="https://github.com/$owner_path/$repo_path/releases/download/$version_path/$checksum_path_segment"
  download_and_install "$archive_url" "$checksum_url" "$archive_asset_name"
}

install_pin() {
  pinned_version=$1
  is_valid_git_tag "$pinned_version" || fail "--version must be a valid Git tag and must not be latest"
  target=$(detect_target) || exit 1
  set -- $target
  os=$1
  arch=$2
  asset_arch_label=$(resolve_asset_arch_label "$os" "$arch") || exit 1
  trap cleanup EXIT
  trap cleanup_on_signal HUP INT TERM
  tmpdir=$(mktemp -d) || fail "failed to create temporary directory"
  archive_path="$tmpdir/archive"
  checksum_path="$tmpdir/checksums"
  extract_dir="$tmpdir/extract"
  archive_asset_name=$(render_archive_asset_name "$pinned_version" "$os" "$asset_arch_label")
  validate_archive_asset_name "$archive_asset_name"
  owner_path=$(url_encode_segment "$OWNER")
  repo_path=$(url_encode_segment "$REPO")
  version_path=$(url_encode_segment "$pinned_version")
  archive_path_segment=$(url_encode_segment "$archive_asset_name")
  checksum_path_segment=$(url_encode_segment "$CHECKSUM_FILE_NAME")
  archive_url="https://github.com/$owner_path/$repo_path/releases/download/$version_path/$archive_path_segment"
  checksum_url="https://github.com/$owner_path/$repo_path/releases/download/$version_path/$checksum_path_segment"
  download_and_install "$archive_url" "$checksum_url" "$archive_asset_name"
}

main "$@"
