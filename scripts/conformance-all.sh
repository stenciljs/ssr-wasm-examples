#!/usr/bin/env bash
# Start each example's image, run the conformance suite against it, stop it.
# Build the images first (scripts/docker-build.sh).
#
#   scripts/conformance-all.sh                 # every example
#   scripts/conformance-all.sh go-nethttp      # just one
set -uo pipefail
cd "$(dirname "$0")/.."

port_for() {
  case "$1" in
    php-laravel) echo 8001 ;; python-django) echo 8002 ;; ruby-rails) echo 8003 ;; go-nethttp) echo 8004 ;;
    rust-axum) echo 8005 ;; java-spring) echo 8006 ;; dotnet-aspnet) echo 8007 ;;
    *) echo "no port for $1 - add it to CONTRACT.md and here" >&2; return 1 ;;
  esac
}

examples=("$@")
if [ ${#examples[@]} -eq 0 ]; then
  for d in examples/*/; do examples+=("$(basename "$d")"); done
fi

failed=()
for name in "${examples[@]}"; do
  port=$(port_for "$name") || { failed+=("$name"); continue; }
  echo "==> $name on :$port"
  docker rm -f "ssr-wasm-$name" >/dev/null 2>&1
  if curl -s -o /dev/null "http://localhost:$port/"; then
    echo "port $port is already in use - stop whatever is on it first" >&2
    failed+=("$name"); continue
  fi
  if ! docker run -d --rm --name "ssr-wasm-$name" -p "$port:$port" "ssr-wasm-$name" >/dev/null; then
    failed+=("$name"); continue
  fi
  for _ in $(seq 1 120); do curl -sf "http://localhost:$port/health" >/dev/null && break; sleep 1; done
  if ! (cd tests/conformance && BASE_URL="http://localhost:$port" npx playwright test --retries=0); then
    failed+=("$name")
    docker logs --tail 50 "ssr-wasm-$name"
  fi
  docker stop "ssr-wasm-$name" >/dev/null
done

if [ ${#failed[@]} -gt 0 ]; then
  echo "FAILED: ${failed[*]}"
  exit 1
fi
echo "all passed: ${examples[*]}"
