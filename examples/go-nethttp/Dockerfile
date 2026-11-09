# syntax=docker/dockerfile:1
#
# Build from the repo root:
#   docker build -f examples/go-nethttp/Dockerfile -t ssr-wasm-go-nethttp .
#   docker run --rm -p 8004:8004 ssr-wasm-go-nethttp
#
# Unit tests:            docker build -f examples/go-nethttp/Dockerfile --target test .
# Behind a TLS-inspecting proxy, pass its root CA for the module download:
#   docker build --secret id=ca,src=/path/to/proxy-ca.pem -f examples/go-nethttp/Dockerfile ...

ARG GO_VERSION=1.27

# ---- build: a static, cgo-free binary (the Extism Go SDK runs wasm on wazero, pure Go) ----
FROM golang:${GO_VERSION}-alpine AS build
WORKDIR /src
ENV CGO_ENABLED=0 GOFLAGS=-trimpath

# Dependencies first, so source changes don't re-download them.
COPY examples/go-nethttp/go.mod examples/go-nethttp/go.sum ./
RUN --mount=type=secret,id=ca,required=false \
    if [ -s /run/secrets/ca ]; then \
      cat /etc/ssl/certs/ca-certificates.crt /run/secrets/ca > /tmp/ca.pem && export SSL_CERT_FILE=/tmp/ca.pem; \
    fi; \
    go mod download

COPY examples/go-nethttp/ ./
RUN --mount=type=cache,target=/root/.cache/go-build \
    go build -ldflags="-s -w" -o /out/server .

COPY packages/components/dist/ssr-wasm/index.wasm /app/ssr-wasm/index.wasm

# ---- test: only built with --target test ----
FROM build AS test
ENV SSR_WASM_PATH=/app/ssr-wasm/index.wasm
RUN --mount=type=cache,target=/root/.cache/go-build \
    go vet ./... && go test -v ./...

# ---- runtime ----
FROM gcr.io/distroless/static-debian12:nonroot
WORKDIR /app
COPY --from=build /out/server /app/server
COPY --from=build /app/ssr-wasm/index.wasm /app/ssr-wasm/index.wasm
COPY packages/components/dist/loader-bundle/demo-components/ /app/stencil/
ENV PORT=8004 \
    SSR_WASM_PATH=/app/ssr-wasm/index.wasm \
    STENCIL_CLIENT_DIR=/app/stencil
EXPOSE 8004
HEALTHCHECK --interval=10s --timeout=5s --start-period=60s CMD ["/app/server", "-healthcheck"]
ENTRYPOINT ["/app/server"]
