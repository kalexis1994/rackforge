#!/usr/bin/env bash
set -euo pipefail

target_hostname="${RACKFORGE_HOSTNAME:-rackforge}"
current_hostname="$(hostnamectl --static)"

if [[ "$current_hostname" != "$target_hostname" ]]; then
  sudo cp -a /etc/hosts /etc/hosts.rackforge-backup
  sudo hostnamectl set-hostname "$target_hostname"
  if grep -q '^127\.0\.1\.1' /etc/hosts; then
    sudo sed -Ei \
      "s/^127\\.0\\.1\\.1.*/127.0.1.1 ${target_hostname}/" \
      /etc/hosts
  else
    printf '127.0.1.1 %s\n' "$target_hostname" |
      sudo tee -a /etc/hosts >/dev/null
  fi
fi

sudo apt-get update
sudo DEBIAN_FRONTEND=noninteractive apt-get install -y \
  --no-install-recommends \
  alsa-utils \
  build-essential \
  ca-certificates \
  cmake \
  curl \
  git \
  jq \
  libasound2-dev \
  libsdl2-dev \
  libudev-dev \
  ninja-build \
  patch \
  pkg-config \
  rustup \
  tmux

rustup default stable

sudo systemctl enable --now ssh

install -d \
  "$HOME/rackforge/current" \
  "$HOME/rackforge/bin" \
  "$HOME/rackforge/build" \
  "$HOME/rackforge/banks" \
  "$HOME/rackforge/performances" \
  "$HOME/rackforge/share/nuked-sc55" \
  "$HOME/rackforge/state" \
  "$HOME/rackforge/logs"
