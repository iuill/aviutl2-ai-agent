#!/usr/bin/env bash
set -euo pipefail

for _ in {1..30}; do
  if /usr/bin/docker info >/dev/null 2>&1; then
    break
  fi
  sleep 1
done

/usr/bin/docker info >/dev/null

if ! /usr/bin/docker buildx inspect agent-builder >/dev/null 2>&1; then
  /usr/bin/docker buildx create \
    --name agent-builder \
    --driver docker-container \
    --driver-opt network=host \
    --buildkitd-flags '--allow-insecure-entitlement network.host'
fi

/usr/bin/docker buildx inspect agent-builder --bootstrap >/dev/null
