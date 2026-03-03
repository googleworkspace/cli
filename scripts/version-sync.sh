#!/usr/bin/env bash
# Syncs the version from package.json into Cargo.toml and updates Cargo.lock.
# Used by changesets/action as a custom version command.
set -euo pipefail

# Run the standard changeset version command first
pnpm changeset version

# Read the new version from package.json
VERSION=$(node -p "require('./package.json').version")

# Update Cargo.toml version field
# Uses awk to only change the version under [package], not other sections
awk -v ver="$VERSION" '
  /^\[package\]/ { in_pkg=1 }
  /^\[/ && !/^\[package\]/ { in_pkg=0 }
  in_pkg && /^version = / { $0 = "version = \"" ver "\"" }
  { print }
' Cargo.toml > Cargo.toml.tmp && mv Cargo.toml.tmp Cargo.toml

# Update Cargo.lock to match
cargo generate-lockfile 2>/dev/null || cargo update -w 2>/dev/null || true

# Stage the changed files so changesets/action commits them
git add Cargo.toml Cargo.lock
