#!/usr/bin/env bash
# Computes the Docker image tags and writes them to $GITHUB_OUTPUT:
#   registry: image name prefix, e.g. ghcr.io/sm-obstacle/obstacle-api
# For each service <svc>:
#   <svc>_version: version of the matching cargo package
#   <svc>_tags:    comma-separated list of full image tags
#
# Images are tagged <version>-<sha7>. When pushing a tag vX.Y.Z where X.Y.Z is
# the game-api version, the suffix is dropped. If that tag is also the highest
# version in the repository, images are tagged "latest" as well.
set -euo pipefail

# Service (Dockerfile stage) -> cargo package it takes its version from.
services="backend:game-api admin:admin socc:socc migrator:migration"

# GHCR requires lowercase names.
repo="${GITHUB_REPOSITORY:-SM-Obstacle/Obstacle-API}"
registry="ghcr.io/$(tr '[:upper:]' '[:lower:]' <<<"$repo")"

versions=$(cargo metadata --no-deps --format-version 1 \
  | jq -c '[.packages[] | {(.name): .version}] | add')
api_version=$(jq -r '."game-api"' <<<"$versions")
suffix="-${GITHUB_SHA::7}"
latest=false

if [[ "${GITHUB_REF_TYPE:-}" == "tag" ]]; then
  tag="$GITHUB_REF_NAME"
  highest=$(git tag -l 'v*' | grep -E '^v[0-9]+\.[0-9]+\.[0-9]+$' | sort -V | tail -n1)

  if [[ "$tag" != "v$api_version" ]]; then
    echo "::warning::Tag $tag does not match the game-api version ($api_version): images are suffixed with the commit."
  else
    suffix=""
    if [[ "$tag" == "$highest" ]]; then
      latest=true
    else
      echo "::notice::Tag $tag is not the most recent one ($highest): no latest tag."
    fi
  fi
fi

echo "registry=$registry" >> "${GITHUB_OUTPUT:-/dev/stdout}"

for entry in $services; do
  svc="${entry%%:*}"
  pkg="${entry#*:}"
  version=$(jq -r --arg p "$pkg" '.[$p] // empty' <<<"$versions")
  if [[ -z "$version" ]]; then
    echo "::error::Cargo package not found: $pkg"
    exit 1
  fi

  tags="$registry/$svc:$version$suffix"
  if [[ "$latest" == true ]]; then
    tags="$tags,$registry/$svc:latest"
  fi

  echo "$svc : $tags"
  {
    echo "${svc}_version=$version"
    echo "${svc}_tags=$tags"
  } >> "${GITHUB_OUTPUT:-/dev/stdout}"
done
