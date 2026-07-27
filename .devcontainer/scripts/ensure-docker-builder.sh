#!/usr/bin/env bash
set -euo pipefail

docker_cli=""
for candidate in /usr/bin/docker /bin/docker /usr/local/bin/docker; do
  if [[ -x "${candidate}" ]]; then
    docker_cli="${candidate}"
    break
  fi
done

if [[ -z "${docker_cli}" ]]; then
  echo "Docker CLI is not installed" >&2
  exit 127
fi

for _ in {1..30}; do
  if "${docker_cli}" info >/dev/null 2>&1; then
    break
  fi
  sleep 1
done

"${docker_cli}" info >/dev/null

if ! "${docker_cli}" buildx inspect agent-builder >/dev/null 2>&1; then
  "${docker_cli}" buildx create \
    --name agent-builder \
    --driver docker-container \
    --driver-opt network=host \
    --buildkitd-flags '--allow-insecure-entitlement network.host'
fi

"${docker_cli}" buildx inspect agent-builder --bootstrap >/dev/null
