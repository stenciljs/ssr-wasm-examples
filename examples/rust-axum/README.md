# Rust Axum + Stencil SSR WASM

Server-renders Stencil components from Rust with [Axum](https://github.com/tokio-rs/axum),
[MiniJinja](https://github.com/mitsuhiko/minijinja) and the
[Extism Rust SDK](https://github.com/extism/extism/tree/main/runtime), which runs the `ssr-wasm`
plugin on [wasmtime](https://wasmtime.dev). No Node.js: the result is one binary.

It implements the shared [contract](../../CONTRACT.md) on port **8005**:

| Route | What it shows | Where |
|---|---|---|
| `GET /` | **Middleware mode**: an `axum::middleware::from_fn` layer buffers the `text/html` response and passes the whole document through `renderToString` | [`src/middleware.rs`](src/middleware.rs) (`ssr_middleware`), page in [`templates/index.html`](templates/index.html) |
| `GET /helper` | **Helper mode**: a MiniJinja filter renders each component as a fragment (`{"fullDocument": false}`); the layout is never sent to the plugin | `stencil` filter in [`src/pages.rs`](src/pages.rs), page in [`templates/helper.html`](templates/helper.html) |
| `GET /health` | `ok`, without touching the plugin | [`src/main.rs`](src/main.rs) |
| `GET /stencil/*` | the client bundle, via `tower_http::services::ServeDir` | [`src/main.rs`](src/main.rs) |

The plugin wrapper (compile once, instance pool, per-request handle) is [`src/ssr.rs`](src/ssr.rs).

```text
Cargo.toml, Cargo.lock
src/main.rs            config, router, graceful shutdown, --warm-cache / --healthcheck
src/ssr.rs             Renderer (CompiledPlugin + pool), Handle, unit tests
src/middleware.rs      middleware mode
src/pages.rs           MiniJinja templates, the `stencil` filter, the two page handlers
templates/             layout.html, index.html, helper.html, components/*.html
wasmtime-cache.toml    compilation cache config used by the image
Dockerfile, Dockerfile.dockerignore
```

## How it works

### The `ssr` module

```rust
let renderer = ssr::Renderer::new(&ssr::Config::new("index.wasm"))?; // compile + fill the pool

let html = renderer
    .run(move |mut h| {                       // waits (async) for a free instance,
        let page = h.render(&page, &Options::new());   // then runs on a blocking thread
        let frag = h.render(&markup, &ssr::fragment()); // {"fullDocument": false}
        page
    })                                        // the Handle drops here: instance back in the pool
    .await?;
```

- **Compile once.** `extism::CompiledPlugin` compiles the module to native code (Cranelift) once
  per process; `Plugin::new_from_compiled` makes instances from it.
- **A pool of instances.** A `Plugin` runs one call at a time, so `Renderer::new` creates
  `SSR_POOL_SIZE` instances (default `std::thread::available_parallelism()`, which respects the
  container's CPU quota) and keeps the idle ones in a `Mutex<Vec<Plugin>>`, guarded by a
  `tokio::sync::Semaphore` with one permit per instance. Taking a permit is async and FIFO, has a
  timeout (30 s, then `503`), and if the request's future is dropped while it waits (client went
  away) it just leaves the queue. Whoever holds a permit can always pop an instance.
- **Plugin calls never run on the async workers.** A render is blocking CPU work (QuickJS
  interpreting the components inside wasm), so `Renderer::run` moves the `Handle` into
  `tokio::task::spawn_blocking`. Only as many blocking threads do plugin work as there are
  instances, because each one holds a permit.
- **One `Handle` per request, owned, not borrowed.** It's `'static` and `Send`, which is what
  lets helper mode hand it to the template engine (below). Its `Drop` puts the instance back in
  the pool (before releasing the permit, so the invariant holds).
- **Hydration ids.** Full-document renders (middleware mode) always number their ids from 1, so
  they need no reset. Fragments share one id sequence in the plugin, so the fragments of a page
  never collide. The handle calls `resetSsrDocData` once, before its first fragment, so fragment
  ids also restart at 1 on every request. That's only for repeatable output: without it ids would
  still be unique, and resetting between fragments would make them collide.
- **Broken instances are replaced.** Every call has a 30 s limit (the manifest's `timeout_ms`,
  enforced by wasmtime epoch interruption). A call that traps or times out may leave QuickJS
  half-way through something, so the handle is marked broken, later renders on it fall back, and
  its `Drop` builds a fresh instance on a separate thread (still holding the permit) instead of
  pooling it. The same happens if the closure panics.
- **Errors fall back.** If `renderToString` returns `{"__error": ...}` or the call fails,
  `Handle::render` logs it and returns the HTML it was given; the components still render on the
  client.
- **New instances are warmed.** Extism instantiates lazily, and the first call into an extism-js
  plugin starts QuickJS and evaluates the bundled components, so each instance gets one
  `resetSsrDocData` call before it goes into the pool.

Extism's Rust SDK has its own `extism::Pool`. It isn't used here because checking out an instance
blocks the thread on a `Condvar`, which is the wrong thing to do on a tokio worker, and it creates
instances lazily, on someone's request.

A dedicated thread per instance (an actor owning its `Plugin`, fed through a channel) would also
work. `spawn_blocking` with a semaphore is less code, and keeps the `Handle` an ordinary value
that can be moved into a template render.

### Middleware mode

`ssr_middleware` is attached with `route_layer` to the `/` route only. It runs the handler, and if
the response is `text/html` (and not encoded, not `204`/`304`) it collects the body with
`axum::body::to_bytes`, renders the document through the pool, drops the old `Content-Length` and
returns the result. Anything else passes through untouched.

### Helper mode, and why MiniJinja

The four components are ordinary template files in [`templates/components/`](templates/components),
so MiniJinja's autoescaping does all the escaping, including the task JSON in
`<demo-task-list tasks="{{ tasks }}">` (`tasks` is a plain string from `serde_json`, so it's
attribute-escaped; nothing is escaped by hand). Middleware mode `{% include %}`s them; helper
mode wraps each include in a filter block:

```jinja
{% filter stencil %}{% include "components/demo-card.html" %}{% endfilter %}
```

The block body is rendered first, with normal escaping. `stencil` then passes that markup to the
request's handle with `{"fullDocument": false}` and returns `Value::from_safe_string(...)`, so the
rendered markup is inlined unescaped. The handle reaches the filter through the render context:
the `/helper` handler wraps it in an `Arc<SsrContext>` (a `Mutex<Handle>` that implements
MiniJinja's `Object`), passes it as the `ssr` variable, and the filter finds it with
`State::lookup("ssr")`. All four calls share one instance and one id sequence. The whole template
render runs inside `Renderer::run`, on the blocking thread.

MiniJinja over Askama, because:

- the helper needs per-request state. A MiniJinja filter receives the template `State`, so it can
  look the handle up in the context, and the templates stay plain HTML with
  `{% filter stencil %}` around a component. An Askama filter would have to be passed the handle
  explicitly at every call site;
- a filter *block* is the natural shape for "render this markup", and the body gets the
  engine's own escaping before the filter sees it;
- no proc-macro code generation, which matters when the dependency build already takes most of an
  hour on one CPU. The templates are still embedded with `include_str!` and parsed at startup,
  with `UndefinedBehavior::Strict`, so a broken template stops the server from starting rather
  than failing a request.

Askama's compile-time checking is the main thing given up. Askama's filter blocks
(`{% filter %}`) would work too, with the handle threaded through as a filter argument.

Both modes send a `Server-Timing: ssr;dur=…` header with the time spent in the plugin.

## Run it

### Docker

The build context is the repo root:

```sh
docker build -f examples/rust-axum/Dockerfile -t ssr-wasm-rust-axum .
docker run --rm -p 8005:8005 ssr-wasm-rust-axum
```

Behind a TLS-inspecting proxy, `cargo fetch` fails with `SSL certificate problem: unable to get
local issuer certificate`. Pass the proxy's root CA as a build secret (it's only used for the
download and isn't stored in the image):

```sh
docker build --secret id=ca,src=/path/to/proxy-root-ca.pem -f examples/rust-axum/Dockerfile -t ssr-wasm-rust-axum .
```

The dependencies (wasmtime and Cranelift are most of them) are fetched and compiled against a stub
`main.rs` in their own layers, so changing the example's source only rebuilds the example. The
first build is slow: about 25 minutes on one CPU, almost all of it wasmtime and Cranelift. After
that, a source change rebuilds in about 20 s. The image is a multi-stage build ending in
`distroless/cc-debian12:nonroot` (glibc, no shell): the binary, the plugin, the client bundle and a
pre-filled wasmtime compilation cache, about 56 MB in all.
[`Dockerfile.dockerignore`](Dockerfile.dockerignore) keeps the build context down to the files it
copies.

Unit tests (they need the plugin, so they run in the build stage):

```sh
docker build -f examples/rust-axum/Dockerfile --target test .
```

### Natively

With Rust 1.96+, from this directory (the default paths point at `packages/components/dist`):

```sh
cargo run --release
cargo test --release
```

Use `--release`: wasmtime built in debug mode compiles the plugin very slowly.

| Env var | Default |
|---|---|
| `PORT` | `8005` |
| `SSR_WASM_PATH` | `../../packages/components/dist/ssr-wasm/index.wasm` |
| `STENCIL_CLIENT_DIR` | `../../packages/components/dist/loader-bundle/demo-components` |
| `SSR_POOL_SIZE` | `available_parallelism()` |
| `SSR_CACHE_CONFIG` | unset (no compilation cache); `/app/wasmtime-cache.toml` in the image |
| `EXTISM_ENABLE_WASI_OUTPUT` | unset (plugin stdout/stderr discarded); `1` in the image |
| `RUST_LOG` | `info` |

Then check it against the conformance suite:

```sh
cd tests/conformance && BASE_URL=http://localhost:8005 npx playwright test
```

## Timings

Rough numbers from a small Docker VM (1 CPU, ~2.5 GB). Render times are medians of 30 sequential
requests on a warm server, measured the same way for every example (see
[Render times](../../README.md#render-times) in the root README). Startup numbers come from the
server's logs.

| | |
|---|---|
| Compile (`CompiledPlugin::new`), no cache | 2.1-3.6 s |
| Compile, wasmtime cache warm (the image) | 70-100 ms |
| Instantiate one instance, including its first call | 0.4-4 ms |
| Render `/` (middleware, whole document) | ~56 ms (~58 ms end to end) |
| Render `/helper` (reset + 4 fragments) | ~75 ms (~77 ms end to end) |
| Memory, idle, pool of 1 / pool of 4 after 16 concurrent requests | ~17-20 MB / ~30 MB |
| Dependency build (Docker, one CPU) | ~25 min |

That's two to three times faster per render than the Go example on wazero, in the same VM.
The plugin is mostly QuickJS interpreting the components, and Cranelift's code for the QuickJS
interpreter is the difference.

### Is the compilation cache worth it?

Here it is. Compiling the ~6.5 MB module with Cranelift takes 2-4 s on one CPU, against about
100 ms to load it from the cache. That doesn't matter much for a long-running server, but it's
most of the container's startup time, so it helps with autoscaling and scale-to-zero. The image
fills the cache at build time (`server --warm-cache`) in `/app/wasmtime-cache`, which is owned by
the runtime user. The cache key includes wasmtime's version, the compiler settings and the CPU
features, so a mismatch is just a miss: the server compiles as usual and writes the result.

## Extism Rust SDK notes

- **The compilation cache is on unless you turn it off.** Unless the builder says otherwise
  (`with_cache_config(path)` or `with_cache_disabled()`), `CompiledPlugin::new` enables wasmtime's
  cache from `$EXTISM_CACHE_CONFIG`, or else in `~/.cache/wasmtime`, and **fails to compile** if it
  can't create that directory (a read-only home, a user without one). `Renderer::new` always picks
  one explicitly: `SSR_CACHE_CONFIG` if it's set, otherwise no cache. The cache is configured with
  a TOML file ([`wasmtime-cache.toml`](wasmtime-cache.toml)), not a directory: Extism overwrites
  any `cache` set through `with_wasmtime_config`.
- **The plugin's stdout/stderr are discarded** unless the `EXTISM_ENABLE_WASI_OUTPUT` env var is
  set, in which case they're inherited by the server. The image sets it, so `console.*` from
  components shows up in `docker logs`. There's no per-instance setting for redirecting them.
- **Instantiation is lazy.** `Plugin::new_from_compiled` only links; the module is instantiated
  on the first `call`. Each new instance gets a `resetSsrDocData` call so that cost (and QuickJS
  start-up) isn't paid on a request.
- **`Plugin` is `Send` and `Sync`**, but `call` takes `&mut self` and fails with "cannot make
  reentrant calls into plugin" if the instance is already busy, so it still needs one owner at a
  time: the pool gives each request exclusive ownership.
- **`extism::Pool` blocks.** Its checkout waits on a `std::sync::Condvar`, so it's not usable as-is
  from async code without `spawn_blocking` around the wait as well. Hence the semaphore pool.
- **Timeouts are cooperative**: `Manifest::with_timeout` uses wasmtime epoch interruption, driven
  by a timer thread the SDK starts. A timed-out call returns an `Error` ("timeout"), and the
  instance is replaced. There's also `Plugin::cancel_handle()` for cancelling from another
  thread; it isn't used for disconnected clients, because a cancelled instance would have to be
  replaced too, which costs more than letting a 50 ms render finish.
- **Feature flags.** The SDK's default features include an HTTP client (`ureq`) for plugins that
  call `http_request`. The ssr-wasm plugin doesn't, so the example builds with
  `default-features = false, features = ["register-filesystem", "wasmtime-default-features"]`.
  `register-filesystem` is what lets a manifest load `Wasm::file`.
- **Build in release mode.** wasmtime/Cranelift in a debug build compile the plugin very slowly.
  The unit tests run with `cargo test --release` too, which also reuses the release dependency
  build in the Docker image.
