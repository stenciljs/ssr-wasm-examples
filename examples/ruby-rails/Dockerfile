# Build from the repo root:
#   docker build -f examples/ruby-rails/Dockerfile -t ssr-wasm-ruby-rails .
#   docker run --rm -p 8003:8003 ssr-wasm-ruby-rails
ARG RUBY_VERSION=3.4

# ---- libextism: the shared library the `extism` gem loads through FFI ----
FROM ruby:${RUBY_VERSION}-slim AS libextism
ARG EXTISM_VERSION=v1.41.0
ARG EXTISM_SHA256=753cef478c853556212f6ad64b6e98adbc8b7908931c54b79678bb7c1fb2643b
RUN apt-get update -qq \
 && apt-get install -y --no-install-recommends curl ca-certificates \
 && curl -fsSL -o /tmp/libextism.tar.gz \
      "https://github.com/extism/extism/releases/download/${EXTISM_VERSION}/libextism-x86_64-unknown-linux-gnu-${EXTISM_VERSION}.tar.gz" \
 && echo "${EXTISM_SHA256}  /tmp/libextism.tar.gz" | sha256sum -c - \
 && tar -xzf /tmp/libextism.tar.gz -C /tmp libextism.so

# ---- gems (puma, nio4r etc. have native extensions, so this stage has a compiler) ----
FROM ruby:${RUBY_VERSION}-slim AS gems
ENV BUNDLE_DEPLOYMENT=1 BUNDLE_PATH=/usr/local/bundle BUNDLE_WITHOUT=development:test
RUN apt-get update -qq \
 && apt-get install -y --no-install-recommends build-essential \
 && rm -rf /var/lib/apt/lists/*
WORKDIR /app
COPY examples/ruby-rails/Gemfile examples/ruby-rails/Gemfile.lock ./
RUN bundle install --jobs 2 \
 && rm -rf "${BUNDLE_PATH}"/ruby/*/cache "${BUNDLE_PATH}"/ruby/*/bundler/gems/*/.git \
 && find "${BUNDLE_PATH}" -name "*.o" -delete

# ---- runtime ----
FROM ruby:${RUBY_VERSION}-slim
ENV RAILS_ENV=production \
    BUNDLE_DEPLOYMENT=1 BUNDLE_PATH=/usr/local/bundle BUNDLE_WITHOUT=development:test \
    SSR_WASM_PATH=/app/stencil-dist/ssr-wasm/index.wasm \
    STENCIL_CLIENT_DIR=/app/stencil-dist/client \
    PORT=8003 RAILS_MAX_THREADS=3 WEB_CONCURRENCY=0
COPY --from=libextism /tmp/libextism.so /usr/local/lib/libextism.so
RUN ldconfig
COPY --from=gems /usr/local/bundle /usr/local/bundle

WORKDIR /app
RUN useradd --system --create-home --uid 1000 rails
# Build artifacts first (they change less often than the app while you're editing it).
COPY packages/components/dist/ssr-wasm/index.wasm stencil-dist/ssr-wasm/index.wasm
COPY packages/components/dist/loader-bundle/demo-components/ stencil-dist/client/
COPY examples/ruby-rails/ ./
USER rails

EXPOSE 8003
CMD ["bundle", "exec", "puma", "-C", "config/puma.rb"]
