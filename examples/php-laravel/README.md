# PHP + Laravel: Stencil `ssr-wasm` example

A minimal Laravel 13 app (PHP 8.4, Laravel Octane on FrankenPHP) that server-renders Stencil
web components by calling the `ssr-wasm` Extism plugin through the
[Extism PHP SDK](https://github.com/extism/php-sdk) (`extism/extism`, which uses PHP's FFI
extension and `libextism.so`). It implements [`CONTRACT.md`](../../CONTRACT.md) on port **8001**.

| Route | What it shows |
|---|---|
| `GET /` | **Middleware mode**: Blade renders plain component tags, then middleware passes the whole `text/html` response through `renderToString` (`fullDocument: true`). |
| `GET /helper` | **Helper mode**: no post-processing. Each top-level component is wrapped in a `@stencil … @endstencil` Blade directive that renders it as a fragment (`fullDocument: false`). |
| `GET /health` | `ok`. Doesn't touch the plugin. |
| `GET /stencil/*` | The client bundle, served from `STENCIL_CLIENT_DIR`. |

## Where things live

| File | Role |
|---|---|
| [`app/Ssr/SsrPlugin.php`](app/Ssr/SsrPlugin.php) | Wraps one Extism plugin instance: loads `index.wasm` with WASI, `resetSsrDocData()`, `renderToString()`. Bound as a **singleton**: one per worker. |
| [`app/Ssr/StencilRenderer.php`](app/Ssr/StencilRenderer.php) | Per-request renderer. Bound as **`scoped`**, so it's created once per request; it calls `resetSsrDocData` once, before the request's first fragment. `render()` falls back to the un-rendered HTML and logs on `__error` (or on an FFI exception). |
| [`app/Providers/AppServiceProvider.php`](app/Providers/AppServiceProvider.php) | Container bindings, and the `@stencil` / `@endstencil` Blade directives (the helper). |
| [`app/Http/Middleware/RenderStencilComponents.php`](app/Http/Middleware/RenderStencilComponents.php) | The middleware. Skips anything that isn't a buffered `text/html` response. Attached to `/` in [`routes/web.php`](routes/web.php). |
| [`resources/views/middleware.blade.php`](resources/views/middleware.blade.php), [`helper.blade.php`](resources/views/helper.blade.php), [`layouts/app.blade.php`](resources/views/layouts/app.blade.php) | The page. Task JSON goes into the attribute with Blade's own escaping: `tasks="{{ json_encode($tasks) }}"`. |
| [`app/Http/Controllers/StencilAssetController.php`](app/Http/Controllers/StencilAssetController.php) | `/stencil/*` static files. |
| [`config/ssr.php`](config/ssr.php), [`config/octane.php`](config/octane.php) | `SSR_WASM_PATH` / `STENCIL_CLIENT_DIR`; Octane warms `SsrPlugin` when each worker boots. |

Helper mode in a template:

```blade
@stencil
  <demo-task-list tasks="{{ json_encode($tasks) }}"></demo-task-list>
@endstencil
```

The enclosed markup is ordinary Blade. Its output is buffered and passed to
`StencilRenderer::fragment()`. Because the renderer is request-scoped, all the fragments on a page share
one ID sequence, reset once before the first fragment, so their hydration IDs don't collide. A Blade directive is used here, not a
component (`<x-stencil>`). A component whose `render()` returns a string has that string
compiled as Blade, which would re-interpret the rendered HTML.

## Plugin lifecycle: Octane (FrankenPHP worker mode), one plugin per worker

Classic PHP-FPM is shared-nothing: every request starts with an empty heap, so it would have to
create the plugin on every request. With Extism's compilation cache warm, that adds ~150 ms of
instantiation per request (a 6.5 MB QuickJS + components module). With the cache cold, it costs
6–10 s to JIT-compile the wasm. On top of that, every concurrent request holds its own plugin
instance in memory.

So this example runs under **Laravel Octane on FrankenPHP**. Each Octane worker is a long-lived
PHP thread that boots Laravel once and then serves requests one at a time:

- `SsrPlugin` is a container **singleton**, and it's listed in `octane.warm`, so each worker
  instantiates the plugin once at boot and reuses it for every request it serves. A worker never
  handles two requests at once, so the instance is never shared between concurrent requests,
  which matters because an Extism instance isn't thread-safe. Concurrency equals the number of workers
  (`OCTANE_WORKERS`, default 2), and you get one plugin per worker.
- `StencilRenderer` is **scoped**. Octane flushes scoped instances between requests, so each
  request gets a new one. Full-document renders (middleware mode) always number their hydration
  IDs from 1, so they need no reset. Fragments share an ID sequence in the plugin, so the renderer
  calls `resetSsrDocData` once, before the request's first fragment. That's only for repeatable
  output: never resetting would still give unique IDs, and resetting between fragments would
  make them collide.
- Workers are recycled after `--max-requests=1000`, which costs one re-instantiation (~150 ms)
  per 1000 requests.
- The Docker build compiles the plugin once (`RUN php -r …`), which fills wasmtime's cache in
  `~/.cache/wasmtime`, so worker boot pays only for instantiation.

The same code also runs without Octane (`php artisan serve`, PHP-FPM). The singleton then lives
for one request, which gives per-request instantiation. It works correctly, just more slowly (see the timings below).

## Timings

Rough numbers from a small Docker VM (1 CPU, ~2.5 GB). Render times are medians of 30 sequential
requests on a warm server, measured the same way for every example (see
[Render times](../../README.md#render-times) in the root README). Startup numbers come from the
server's logs.

| What | Time |
|---|---|
| Compile plugin (wasmtime cache cold, done once at `docker build`) | 5.8–9.9 s |
| Instantiate plugin (cache warm; once per Octane worker) | ~160 ms |
| `GET /health`, Laravel baseline with no rendering (Octane) | 5 ms |
| **`GET /`**, Octane (1 whole-document `renderToString`) | **~46 ms** |
| **`GET /helper`**, Octane (4 fragment `renderToString` calls) | **~59 ms** |
| `GET /` without Octane (`php artisan serve`, plugin instantiated per request) | 147 ms |
| `GET /helper` without Octane (plugin instantiated per request) | ~360 ms |

The same calls from a standalone PHP CLI script with a fresh plugin took ~85 ms for the whole
page and ~140 ms for the 4 fragments, even after 30 warm-up iterations. That's slower than inside a
long-lived Octane worker, so a plugin that has served many requests appears to render faster. The
per-request rows also include that cold-instance penalty.

Memory: the container uses ~160 MiB with 2 workers, each holding a plugin.

## Run with Docker

The build context is the **repo root**:

```sh
docker build -f examples/php-laravel/Dockerfile -t ssr-wasm-php-laravel .
docker run --rm -p 8001:8001 ssr-wasm-php-laravel
# http://localhost:8001/ and http://localhost:8001/helper

cd tests/conformance && BASE_URL=http://localhost:8001 npx playwright test
```

The image is `dunglas/frankenphp:1-php8.4-bookworm` plus the `ffi` extension, Composer and
`libextism.so` (from the [extism/extism releases](https://github.com/extism/extism/releases),
`EXTISM_VERSION` build arg). The Dockerfile copies `packages/components/dist/ssr-wasm/index.wasm`
and `packages/components/dist/loader-bundle/demo-components/` into `/app/stencil-dist/` and sets
`SSR_WASM_PATH` / `STENCIL_CLIENT_DIR`. Config isn't cached, so both can be overridden with
`docker run -e`.

If your network uses a TLS-inspecting proxy, pass its root CA as a build secret:
`docker build --secret id=ca,src=/path/to/proxy-ca.pem ...`, or set `EXTRA_CA_CERT` and use
`scripts/docker-build.sh php-laravel` from the repo root.

## Run natively

Requirements:

- PHP 8.3+ with the `ffi` extension and `ffi.enable=true` **in php.ini**. The default, `preload`,
  only allows `FFI::cdef()` on the CLI SAPI, so FrankenPHP, PHP-FPM and `artisan serve` (which
  forks `php -S`) would fail. A `-d` flag isn't inherited by `artisan serve`'s child process.
- `libextism` in `/usr/local/lib` or `/usr/lib`. Those are the only places the PHP SDK looks.
  Use `libextism.so` on Linux and `libextism.dylib` on macOS.
- Composer.

```sh
cd examples/php-laravel
composer install
cp .env.example .env && php artisan key:generate

# Octane + FrankenPHP (recommended). The `frankenphp` binary must be on PATH and its embedded
# PHP needs FFI (check `frankenphp php-cli -m`). Octane's auto-downloaded static binary may not
# have it.
php artisan octane:frankenphp --port=8001

# or, without Octane (one plugin instance per request)
php artisan serve --port=8001
```

`SSR_WASM_PATH` and `STENCIL_CLIENT_DIR` default to the build artifacts in this checkout
(`../../packages/components/dist/...`). Build those first with `pnpm build` at the repo root.

## Extism PHP SDK notes

- The SDK keeps its FFI handle in a **global variable named `$lib`** (`global $lib` in
  `Plugin::__construct`). Don't use a global `$lib` of your own.
- The SDK searches only `/usr/local/lib` and `/usr/lib` for the shared library, and that can't be
  configured.
- `Plugin::call()` returns the raw output bytes as a string. The plugin's own errors come back as
  `{"__error": …}` JSON rather than as exceptions. Trap failures (bad function name, wasm trap)
  throw `Extism\FunctionCallException`. `StencilRenderer` handles both.
