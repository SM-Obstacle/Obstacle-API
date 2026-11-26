#!/usr/bin/env bash
# Computes the Docker image tags and writes them to $GITHUB_OUTPUT:
#   registry: image name prefix, e.g. ghcr.io/sm-obstacle/obstacle-api
#   version:  workspace version (from [workspace.package] in Cargo.toml)
#   latest:   "true" if the images are tagged "latest"
#   <svc>_tags: comma-separated list of full image tags, for each service <svc>
#
# All images share the workspace version and are tagged <version>-<sha7>. When
# pushing a tag vX.Y.Z where X.Y.Z is that version, the suffix is dropped. If
# that tag is also the highest version in the repository, images are tagged
# "latest" as well.
set -euo pipefail

services="backend admin socc migrator"

# GHCR requires lowercase names.
repo="${GITHUB_REPOSITORY:-SM-Obstacle/Obstacle-API}"
registry="ghcr.io/$(tr '[:upper:]' '[:lower:]' <<<"$repo")"

# Every crate inherits the workspace version, so game-api's is the workspace one.
version=$(cargo metadata --no-deps --format-version 1 \
  | jq -r '.packages[] | select(.name == "game-api") | .version')
suffix="-${GITHUB_SHA::7}"
latest=false

if [[ "${GITHUB_REF_TYPE:-}" == "tag" ]]; then
  tag="$GITHUB_REF_NAME"
  highest=$(git tag -l 'v*' | grep -E '^v[0-9]+\.[0-9]+\.[0-9]+$' | sort -V | tail -n1)

  if [[ "$tag" != "v$version" ]]; then
    echo "::warning::Tag $tag does not match the workspace version ($version): images are suffixed with the commit."
  else
    suffix=""
    if [[ "$tag" == "$highest" ]]; then
      latest=true
    else
      echo "::notice::Tag $tag is not the most recent one ($highest): no latest tag."
    fi
  fi
fi

{
  echo "registry=$registry"
  echo "version=$version"
  echo "latest=$latest"
} >> "${GITHUB_OUTPUT:-/dev/stdout}"

for svc in $services; do
  tags="$registry/$svc:$version$suffix"
  if [[ "$latest" == true ]]; then
    tags="$tags,$registry/$svc:latest"
  fi

  echo "$svc : $tags"
  echo "${svc}_tags=$tags" >> "${GITHUB_OUTPUT:-/dev/stdout}"
done
