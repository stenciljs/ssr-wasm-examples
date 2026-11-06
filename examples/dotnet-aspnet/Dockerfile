# syntax=docker/dockerfile:1
#
# Build from the repo root:
#   docker build -f examples/dotnet-aspnet/Dockerfile -t ssr-wasm-dotnet-aspnet .
#   docker run --rm -p 8007:8007 ssr-wasm-dotnet-aspnet
#
# Unit tests:            docker build -f examples/dotnet-aspnet/Dockerfile --target test .
# Behind a TLS-inspecting proxy, pass its root CA for the NuGet restore:
#   docker build --secret id=ca,src=/path/to/proxy-ca.pem -f examples/dotnet-aspnet/Dockerfile ...

ARG DOTNET_VERSION=10.0

# ---- build ----
FROM mcr.microsoft.com/dotnet/sdk:${DOTNET_VERSION}-noble AS build
WORKDIR /src

# Packages first, in their own layer, so source changes don't re-download them.
# .NET on Linux reads its root certificates through OpenSSL, which honours SSL_CERT_FILE.
COPY examples/dotnet-aspnet/DotnetAspnet.csproj ./
RUN --mount=type=secret,id=ca,required=false \
    if [ -s /run/secrets/ca ]; then \
      cat /etc/ssl/certs/ca-certificates.crt /run/secrets/ca > /tmp/ca.pem && export SSL_CERT_FILE=/tmp/ca.pem; \
    fi; \
    dotnet restore -r linux-x64 -p:ExtismRuntime=linux-x64; \
    status=$?; rm -f /tmp/ca.pem; exit $status

COPY examples/dotnet-aspnet/ ./
RUN dotnet publish -c Release -r linux-x64 --self-contained false -p:ExtismRuntime=linux-x64 \
      --no-restore -o /out \
 && rm -f /out/*.pdb

# ---- test: only built with --target test ----
FROM build AS test
COPY packages/components/dist/ssr-wasm/index.wasm /app/ssr-wasm/index.wasm
COPY packages/components/dist/loader-bundle/demo-components/ /app/stencil/
ENV SSR_WASM_PATH=/app/ssr-wasm/index.wasm \
    STENCIL_CLIENT_DIR=/app/stencil
RUN --mount=type=secret,id=ca,required=false \
    if [ -s /run/secrets/ca ]; then \
      cat /etc/ssl/certs/ca-certificates.crt /run/secrets/ca > /tmp/ca.pem && export SSL_CERT_FILE=/tmp/ca.pem; \
    fi; \
    dotnet restore tests -p:ExtismRuntime=linux-x64; \
    status=$?; rm -f /tmp/ca.pem; exit $status
RUN dotnet test tests --no-restore -p:ExtismRuntime=linux-x64 --logger "console;verbosity=normal"

# ---- runtime: Ubuntu chiseled (no shell, no package manager), runs as the non-root `app` user ----
FROM mcr.microsoft.com/dotnet/aspnet:${DOTNET_VERSION}-noble-chiseled
WORKDIR /app
COPY --from=build /out/ /app/
COPY packages/components/dist/ssr-wasm/index.wasm /app/ssr-wasm/index.wasm
COPY packages/components/dist/loader-bundle/demo-components/ /app/stencil/
ENV ASPNETCORE_HTTP_PORTS=8007 \
    SSR_WASM_PATH=/app/ssr-wasm/index.wasm \
    STENCIL_CLIENT_DIR=/app/stencil \
    HOME=/home/app
USER $APP_UID

# libextism turns on wasmtime's compilation cache, under $HOME/.cache/wasmtime. Compile the plugin
# once now, as the runtime user, so the server starts from a warm cache instead of spending
# seconds in Cranelift. The cache key includes the CPU features; on a different CPU it's just a
# miss and the server compiles as usual (the directory is writable by `app`).
RUN ["dotnet", "/app/server.dll", "--warm-cache"]

EXPOSE 8007
HEALTHCHECK --interval=30s --timeout=5s --start-period=60s CMD ["dotnet", "/app/server.dll", "--healthcheck"]
ENTRYPOINT ["dotnet", "/app/server.dll"]
