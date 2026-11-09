# syntax=docker/dockerfile:1
# Build from the repo root:
#   docker build -f examples/php-laravel/Dockerfile -t ssr-wasm-php-laravel .
# Behind a TLS-inspecting proxy, pass its root CA (or use scripts/docker-build.sh with EXTRA_CA_CERT):
#   docker build --secret id=ca,src=/path/to/proxy-ca.pem -f examples/php-laravel/Dockerfile ...
FROM dunglas/frankenphp:1-php8.4-bookworm AS base

# Optional extra root CA. Composer and the extension installer download in several steps, so it's
# added to the image's trust store rather than mounted per step.
RUN --mount=type=secret,id=ca,required=false \
    if [ -s /run/secrets/ca ]; then \
      cp /run/secrets/ca /usr/local/share/ca-certificates/extra-ca.crt && update-ca-certificates; \
    fi

# FFI (for the Extism PHP SDK) + Composer, then libextism from the extism/extism releases.
ARG EXTISM_VERSION=v1.41.0
RUN install-php-extensions ffi pcntl @composer \
 && apt-get update && apt-get install -y --no-install-recommends unzip && rm -rf /var/lib/apt/lists/* \
 && curl -fsSL "https://github.com/extism/extism/releases/download/${EXTISM_VERSION}/libextism-x86_64-unknown-linux-gnu-${EXTISM_VERSION}.tar.gz" \
    | tar xz -C /usr/local/lib libextism.so \
 && ldconfig

FROM base

# php.ini: production defaults, and FFI enabled for the web SAPI (the default, "preload", only
# allows FFI::cdef() on the CLI).
RUN cp "$PHP_INI_DIR/php.ini-production" "$PHP_INI_DIR/php.ini" \
 && printf 'ffi.enable=true\nmemory_limit=256M\n' > "$PHP_INI_DIR/conf.d/zz-app.ini" \
 && useradd --create-home --uid 1000 app \
 && chown -R app:app /config/caddy /data/caddy

WORKDIR /app
RUN chown app:app /app
USER app

# Dependencies first: this layer is reused until composer.json/lock change.
COPY --chown=app:app examples/php-laravel/composer.json examples/php-laravel/composer.lock ./
RUN composer install --no-dev --no-scripts --no-autoloader --prefer-dist --no-interaction --no-progress \
 && rm -rf ~/.composer/cache ~/.cache/composer

# The ssr-wasm plugin and the client bundle (CONTRACT.md inputs).
ENV SSR_WASM_PATH=/app/stencil-dist/ssr-wasm/index.wasm \
    STENCIL_CLIENT_DIR=/app/stencil-dist/client
COPY --chown=app:app packages/components/dist/ssr-wasm/index.wasm stencil-dist/ssr-wasm/index.wasm
COPY --chown=app:app packages/components/dist/loader-bundle/demo-components/ stencil-dist/client/

# Extism enables wasmtime's compilation cache (~/.cache/wasmtime). Compile the plugin once at
# build time, so each worker only has to instantiate it at startup instead of JIT-compiling
# 6 MB of wasm.
RUN php -d ffi.enable=true -r 'require "vendor/extism/extism/src/Internal/LibExtism.php"; \
      spl_autoload_register(fn ($c) => require "vendor/extism/extism/src/".str_replace(["Extism\\", "\\"], ["", "/"], $c).".php"); \
      new Extism\Plugin(new Extism\Manifest(new Extism\Manifest\PathWasmSource(getenv("SSR_WASM_PATH"))), true);'

# The app itself.
COPY --chown=app:app examples/php-laravel/ ./
RUN composer dump-autoload --optimize --no-dev --no-interaction \
 && sed -e 's/^APP_ENV=.*/APP_ENV=production/' -e 's/^APP_DEBUG=.*/APP_DEBUG=false/' -e 's/^LOG_LEVEL=.*/LOG_LEVEL=info/' .env.example > .env \
 && php artisan key:generate --force \
 && php artisan route:cache && php artisan view:cache \
 && rm -rf Dockerfile ~/.cache/composer
# (No config:cache, so SSR_WASM_PATH / STENCIL_CLIENT_DIR can still be overridden with `docker run -e`.)

EXPOSE 8001
# One Octane worker = one PHP thread in FrankenPHP = one plugin instance.
ENV OCTANE_WORKERS=2
CMD php artisan octane:frankenphp --host=0.0.0.0 --port=8001 --workers=${OCTANE_WORKERS} --max-requests=1000 --log-level=info
