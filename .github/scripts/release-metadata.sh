#!/usr/bin/env bash
# SPDX-License-Identifier: GPL-3.0-or-later
# Copyright (C) 2026 Nyabi (nyabi-gh)

# Writes Ugurugu-<NAME>-build-metadata.txt for the macOS and Linux release
# jobs, with the fields the Windows job writes.
#
#   bash .github/scripts/release-metadata.sh macOS-arm64 arm64 artifacts

set -euo pipefail

name=$1
architecture=$2
folder=$3

version=$(sed -nE '/^\[workspace\.package\]/,/^\[/ s/^version = "([^"]+)"/\1/p' Cargo.toml)
if [[ -z "$version" ]]; then
  echo "Could not read the workspace version"
  exit 1
fi
cat > "$folder/Ugurugu-$name-build-metadata.txt" <<EOF
project=Ugurugu
version=$version
git_commit=$GITHUB_SHA
architecture=$architecture
rustc=$(rustc --version)
velopack_version=$VELOPACK_VERSION
EOF
