#!/usr/bin/env bash
set -euo pipefail

install_packages() {
  local source_list="$1"
  shift
  local apt_options=(
    -o "Dir::Etc::sourcelist=$source_list"
    -o Dir::Etc::sourceparts=-
    -o Acquire::Retries=0
    -o Acquire::http::Timeout=15
    -o Acquire::https::Timeout=15
  )

  sudo apt-get "${apt_options[@]}" -o APT::Update::Error-Mode=any update &&
    sudo apt-get "${apt_options[@]}" install -y --no-install-recommends "$@"
}

if install_packages /etc/apt/sources.list.d/ubuntu.sources "$@"; then
  exit 0
fi

# The runner's default x64 mirror list and ARM64 archive can fail independently.
case "$(dpkg --print-architecture)" in
  amd64) archive=https://mirrors.rit.edu/ubuntu ;;
  arm64) archive=https://mirrors.rit.edu/ubuntu-ports ;;
  *) echo "No Ubuntu mirror fallback for this architecture" >&2; exit 1 ;;
esac

. /etc/os-release
source_list="$(mktemp --suffix=.sources)"
trap 'rm -f -- "$source_list"' EXIT
cat > "$source_list" <<EOF
Types: deb
URIs: $archive
Suites: $VERSION_CODENAME $VERSION_CODENAME-updates $VERSION_CODENAME-security
Components: main restricted universe multiverse
Signed-By: /usr/share/keyrings/ubuntu-archive-keyring.gpg
EOF
chmod a+r "$source_list"

echo "Retrying Ubuntu dependency installation with $archive"
install_packages "$source_list" "$@"
