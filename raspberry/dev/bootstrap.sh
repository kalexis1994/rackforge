#!/usr/bin/env bash
set -euo pipefail

target_hostname="${ARTUPY_HOSTNAME:-artupy}"
current_hostname="$(hostnamectl --static)"

if [[ "$current_hostname" != "$target_hostname" ]]; then
  sudo cp -a /etc/hosts /etc/hosts.artupy-backup
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
  "$HOME/artupy/current" \
  "$HOME/artupy/bin" \
  "$HOME/artupy/build" \
  "$HOME/artupy/banks" \
  "$HOME/artupy/performances" \
  "$HOME/artupy/share/nuked-sc55" \
  "$HOME/artupy/state" \
  "$HOME/artupy/logs"
