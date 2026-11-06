#!/usr/bin/env bash
# Build one example's image (or all of them) from the repo root.
#
#   scripts/docker-build.sh python-django
#   scripts/docker-build.sh            # every example
#
# Behind a TLS-inspecting proxy, set EXTRA_CA_CERT to its root CA (PEM); it's passed to the
# build as the optional `ca` secret that the Dockerfiles accept.
set -euo pipefail
cd "$(dirname "$0")/.."

if [ ! -f packages/components/dist/ssr-wasm/index.wasm ]; then
  echo "packages/components/dist is missing - run 'pnpm build' first" >&2
  exit 1
fi

examples=("$@")
if [ ${#examples[@]} -eq 0 ]; then
  for d in examples/*/; do examples+=("$(basename "$d")"); done
fi

secret=()
if [ -n "${EXTRA_CA_CERT:-}" ]; then secret=(--secret "id=ca,src=$EXTRA_CA_CERT"); fi

for name in "${examples[@]}"; do
  echo "==> ssr-wasm-$name"
  DOCKER_BUILDKIT=1 docker build ${secret[@]+"${secret[@]}"} -f "examples/$name/Dockerfile" -t "ssr-wasm-$name" .
done
