# Rust + Axum

The demo app in Rust, using [Axum](https://github.com/tokio-rs/axum) and
[MiniJinja](https://github.com/mitsuhiko/minijinja). It renders components with the
[Extism Rust SDK](https://github.com/extism/extism/tree/main/runtime) (wasmtime) and builds to a
single binary. Port **8005**.

## Run it

### Docker

Build from the repo root:

```sh
docker build -f examples/rust-axum/Dockerfile -t ssr-wasm-rust-axum .
docker run --rm -p 8005:8005 ssr-wasm-rust-axum
open http://localhost:8005
```

The first build takes about 25 minutes on one CPU, almost all of it compiling wasmtime and
Cranelift. Dependencies are cached in their own layer, so later source changes rebuild in about 20 s.

Behind a TLS-inspecting proxy, `cargo fetch` fails with `SSL certificate problem: unable to get
local issuer certificate`. Pass the proxy's root CA as a build secret (it isn't stored in the image):

```sh
docker build --secret id=ca,src=/path/to/proxy-root-ca.pem -f examples/rust-axum/Dockerfile -t ssr-wasm-rust-axum .
```

Unit tests need the plugin, so they run in the build stage:

```sh
docker build -f examples/rust-axum/Dockerfile --target test .
```

### Natively

Requires Rust 1.96+ and `pnpm build` at the repo root. From this directory:

```sh
cargo run --release
cargo test --release
```

Use `--release`: a debug build of wasmtime compiles the plugin very slowly.

| Env var | Default |
|---|---|
| `PORT` | `8005` |
| `SSR_WASM_PATH` | `../../packages/components/dist/ssr-wasm/index.wasm` |
| `STENCIL_CLIENT_DIR` | `../../packages/components/dist/loader-bundle/demo-components` |
| `SSR_POOL_SIZE` | `available_parallelism()` |
| `SSR_CACHE_CONFIG` | unset (no compilation cache); `/app/wasmtime-cache.toml` in the image |
| `EXTISM_ENABLE_WASI_OUTPUT` | unset (plugin stdout/stderr discarded); `1` in the image |
| `RUST_LOG` | `info` |

Check it against the conformance suite:

```sh
cd tests/conformance && BASE_URL=http://localhost:8005 npx playwright test
```

## Where things are

| Path | What it does |
|---|---|
| [`src/ssr.rs`](src/ssr.rs) | Plugin wrapper: `Renderer` (compiled plugin + instance pool), per-request `Handle`, unit tests |
| [`src/middleware.rs`](src/middleware.rs) | `ssr_middleware`, the `/` middleware that renders the whole HTML response |
| [`src/pages.rs`](src/pages.rs) | MiniJinja setup, the `stencil` filter used on `/helper`, both page handlers |
| [`src/main.rs`](src/main.rs) | Config, router, `/health`, `/stencil/*` static files, `--warm-cache` / `--healthcheck` |
| [`templates/`](templates) | `layout.html`, `index.html`, `helper.html`, and `components/*.html` |
| [`wasmtime-cache.toml`](wasmtime-cache.toml) | Compilation cache config used by the image |

On `/helper`, each component include is wrapped in a filter block:

```jinja
{% filter stencil %}{% include "components/demo-card.html" %}{% endfilter %}
```

MiniJinja escapes the body as usual (including the task JSON), then the filter renders it with
`{"fullDocument": false}` and returns it as safe HTML.

## How it uses the plugin

- **Compile once.** `extism::CompiledPlugin` compiles the module once per process;
  `Plugin::new_from_compiled` creates instances from it.
- **Instance pool.** A `Plugin` runs one call at a time, so `Renderer::new` creates `SSR_POOL_SIZE`
  instances guarded by a `tokio::sync::Semaphore`. Waiting for an instance is async and times out
  after 30 s with a `503`.
- **Off the async workers.** Renders are blocking CPU work, so `Renderer::run` runs them in
  `tokio::task::spawn_blocking`.
- **One `Handle` per request.** Dropping it returns the instance to the pool. On `/helper` the whole
  template render runs with that handle, so all fragments share one instance.
- **`resetSsrDocData`** is called once per request, before the handle's first fragment, so
  fragment hydration IDs start at 1. Each new instance also gets one call before joining the pool,
  to warm it up.
- **On `__error`** or a failed call, `Handle::render` logs it and returns the original HTML; the
  components still render on the client. A call that traps, times out (30 s) or panics gets its
  instance replaced.

Both routes send a `Server-Timing: ssr;dur=…` header with the time spent in the plugin.

## Things to know

- **Set the cache config explicitly.** By default `CompiledPlugin::new` uses a wasmtime cache in
  `~/.cache/wasmtime` and fails to compile if it can't create that directory. `Renderer::new`
  always picks one: `SSR_CACHE_CONFIG` if set, otherwise no cache. Configure it with a TOML file,
  since Extism overwrites any `cache` set through `with_wasmtime_config`.
- **The compilation cache saves 2-4 s of startup.** The image fills it at build time
  (`server --warm-cache`) in `/app/wasmtime-cache`. A version or CPU mismatch is just a cache miss.
- **Plugin stdout/stderr are discarded** unless `EXTISM_ENABLE_WASI_OUTPUT` is set. The image sets
  it, so component `console.*` output shows in `docker logs`.
- **`extism::Pool` blocks the thread** while checking out an instance, which stalls a tokio worker.
  That's why this example uses its own semaphore pool.
- **`Plugin::call` takes `&mut self`** and fails with "cannot make reentrant calls into plugin" if
  the instance is busy, so each instance needs one owner at a time.
- **Feature flags.** The SDK's default features include an HTTP client the plugin doesn't need.
  This example uses `default-features = false, features = ["register-filesystem",
  "wasmtime-default-features"]`; `register-filesystem` is needed to load `Wasm::file`.

## Timings

Rough numbers from a small Docker VM (1 CPU); see [Render times](../../README.md#render-times).

| | |
|---|---|
| Compile (`CompiledPlugin::new`), no cache | 2.1-3.6 s |
| Compile, wasmtime cache warm (the image) | 70-100 ms |
| Instantiate one instance, including its first call | 0.4-4 ms |
| Render `/` (middleware, whole document) | ~36 ms (~38 ms end to end) |
| Render `/helper` (reset + 4 fragments) | ~42 ms (~44 ms end to end) |
| Memory, idle, pool of 1 / pool of 4 after 16 concurrent requests | ~17-20 MB / ~30 MB |
| Image size | ~56 MB |
| Dependency build (Docker, one CPU) | ~25 min |
