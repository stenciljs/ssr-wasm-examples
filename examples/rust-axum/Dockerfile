# syntax=docker/dockerfile:1
#
# Build from the repo root:
#   docker build -f examples/rust-axum/Dockerfile -t ssr-wasm-rust-axum .
#   docker run --rm -p 8005:8005 ssr-wasm-rust-axum
#
# Unit tests:            docker build -f examples/rust-axum/Dockerfile --target test .
# Behind a TLS-inspecting proxy, pass its root CA for the crate download:
#   docker build --secret id=ca,src=/path/to/proxy-ca.pem -f examples/rust-axum/Dockerfile ...

ARG RUST_VERSION=1.99

# ---- deps: download and compile the dependencies (wasmtime is most of it) in their own layers ----
FROM rust:${RUST_VERSION}-slim-bookworm AS deps
WORKDIR /src
ENV CARGO_TERM_COLOR=never

COPY examples/rust-axum/Cargo.toml examples/rust-axum/Cargo.lock ./
RUN --mount=type=secret,id=ca,required=false \
    if [ -s /run/secrets/ca ]; then \
      cat /etc/ssl/certs/ca-certificates.crt /run/secrets/ca > /tmp/ca.pem && export CARGO_HTTP_CAINFO=/tmp/ca.pem; \
    fi; \
    cargo fetch --locked

# Build the dependency graph against a stub main, so source changes don't recompile wasmtime
# (that takes the better part of an hour on one CPU). Same idea as cargo-chef, without the tool.
RUN mkdir src && echo 'fn main() {}' > src/main.rs \
    && cargo build --release --locked --offline \
    && rm -rf src target/release/server target/release/deps/server-* target/release/.fingerprint/rust-axum-*

# ---- build ----
FROM deps AS build
COPY examples/rust-axum/ ./
RUN touch src/main.rs && cargo build --release --locked --offline && cp target/release/server /usr/local/bin/server

COPY packages/components/dist/ssr-wasm/index.wasm /app/ssr-wasm/index.wasm
COPY examples/rust-axum/wasmtime-cache.toml /app/wasmtime-cache.toml

# Pre-fill wasmtime's compilation cache, so the container skips compiling the module to native
# code at startup. The cache key includes the wasmtime version, the module and the CPU features,
# so on a different CPU it's simply a miss: the server compiles and refills it.
RUN SSR_WASM_PATH=/app/ssr-wasm/index.wasm SSR_CACHE_CONFIG=/app/wasmtime-cache.toml server --warm-cache

# ---- test: only built with --target test ----
FROM build AS test
ENV SSR_WASM_PATH=/app/ssr-wasm/index.wasm SSR_CACHE_CONFIG=/app/wasmtime-cache.toml
RUN cargo test --release --locked --offline

# ---- runtime: glibc + libgcc, no shell, uid 65532 ----
FROM gcr.io/distroless/cc-debian12:nonroot
WORKDIR /app
COPY --from=build /usr/local/bin/server /app/server
COPY --from=build /app/ssr-wasm/index.wasm /app/ssr-wasm/index.wasm
COPY --from=build /app/wasmtime-cache.toml /app/wasmtime-cache.toml
COPY --from=build --chown=nonroot:nonroot /app/wasmtime-cache /app/wasmtime-cache
COPY packages/components/dist/loader-bundle/demo-components/ /app/stencil/
# EXTISM_ENABLE_WASI_OUTPUT: without it the SDK discards the plugin's stdout/stderr
# (console.* in components); with it they go to the server's.
ENV PORT=8005 \
    SSR_WASM_PATH=/app/ssr-wasm/index.wasm \
    STENCIL_CLIENT_DIR=/app/stencil \
    SSR_CACHE_CONFIG=/app/wasmtime-cache.toml \
    EXTISM_ENABLE_WASI_OUTPUT=1
EXPOSE 8005
HEALTHCHECK --interval=10s --timeout=5s --start-period=60s CMD ["/app/server", "--healthcheck"]
ENTRYPOINT ["/app/server"]
