#!/usr/bin/env bash
# Creates and pushes a git tag based on the version in package.json.
# Idempotent — skips if the tag already exists.
# Used by changesets/action as the publish command.
set -euo pipefail

VERSION=$(node -p "require('./package.json').version")
TAG="v${VERSION}"

if git rev-parse "$TAG" >/dev/null 2>&1 || git ls-remote --exit-code --tags origin "$TAG" >/dev/null 2>&1; then
  echo "Tag $TAG already exists, skipping"
  exit 0
fi

echo "Creating tag $TAG"
git tag "$TAG"
git push origin "$TAG"
