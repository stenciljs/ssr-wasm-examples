# PHP + Laravel

A Laravel 13 app (PHP 8.4, Octane on FrankenPHP) that server-renders the demo components through
the [Extism PHP SDK](https://github.com/extism/php-sdk) (`extism/extism`, which uses PHP's FFI
extension and `libextism`). Runs on port 8001.

![PHP / Laravel: the demo page, server-rendered by this example](../../docs/screenshots/php-laravel.png)

The demo page as this example's server sends it, with JavaScript disabled in the browser.

## Run it

With Docker, building from the repo root:

```sh
docker build -f examples/php-laravel/Dockerfile -t ssr-wasm-php-laravel .
docker run --rm -p 8001:8001 ssr-wasm-php-laravel
# http://localhost:8001/ and http://localhost:8001/helper

cd tests/conformance && BASE_URL=http://localhost:8001 npx playwright test
```

`SSR_WASM_PATH`, `STENCIL_CLIENT_DIR` and `OCTANE_WORKERS` (default 2) can be overridden with
`docker run -e`.

Behind a TLS-inspecting proxy, pass its root CA as a build secret:
`docker build --secret id=ca,src=/path/to/proxy-ca.pem ...`, or set `EXTRA_CA_CERT` and run
`scripts/docker-build.sh php-laravel` from the repo root.

Natively, you need:

- PHP 8.3+ with the `ffi` extension and `ffi.enable=true` in php.ini (see below).
- `libextism.so` (Linux) or `libextism.dylib` (macOS) in `/usr/local/lib` or `/usr/lib`, from
  the [extism/extism releases](https://github.com/extism/extism/releases).
- Composer, and the components built with `pnpm build` at the repo root.

```sh
cd examples/php-laravel
composer install
cp .env.example .env && php artisan key:generate

# Octane + FrankenPHP (recommended). The `frankenphp` binary must be on PATH and its embedded
# PHP needs FFI (check `frankenphp php-cli -m`).
php artisan octane:frankenphp --port=8001

# or, without Octane (one plugin instance per request)
php artisan serve --port=8001
```

`SSR_WASM_PATH` and `STENCIL_CLIENT_DIR` default to `../../packages/components/dist/...` in this
checkout.

## Where things are

| File | What it does |
|---|---|
| [`app/Ssr/SsrPlugin.php`](app/Ssr/SsrPlugin.php) | Plugin wrapper: loads `index.wasm` with WASI, exposes `resetSsrDocData()` and `renderToString()`. |
| [`app/Ssr/StencilRenderer.php`](app/Ssr/StencilRenderer.php) | Per-request renderer used by both modes; handles errors. |
| [`app/Http/Middleware/RenderStencilComponents.php`](app/Http/Middleware/RenderStencilComponents.php) | Middleware for `/`. Renders buffered `text/html` responses, skips everything else. |
| [`app/Providers/AppServiceProvider.php`](app/Providers/AppServiceProvider.php) | Container bindings and the `@stencil` / `@endstencil` Blade directives (helper for `/helper`). |
| [`routes/web.php`](routes/web.php) | Routes; attaches the middleware to `/`. |
| [`resources/views/`](resources/views/) | `middleware.blade.php`, `helper.blade.php`, `layouts/app.blade.php`. |
| [`app/Http/Controllers/StencilAssetController.php`](app/Http/Controllers/StencilAssetController.php) | Serves the client bundle at `/stencil/*`. |
| [`config/ssr.php`](config/ssr.php), [`config/octane.php`](config/octane.php) | Paths to the plugin and client bundle; Octane warms `SsrPlugin` at worker boot. |

Helper mode in a template:

```blade
@stencil
  <demo-task-list tasks="{{ json_encode($tasks) }}"></demo-task-list>
@endstencil
```

## How it uses the plugin

- **One plugin per Octane worker.** `SsrPlugin` is a container singleton listed in `octane.warm`,
  so each worker creates it once at boot. A worker handles one request at a time, so the
  instance (which isn't thread-safe) is never shared.
- **Compiled once at build time.** The Dockerfile runs the plugin once during `docker build` to
  fill wasmtime's cache in `~/.cache/wasmtime`, so worker boot only pays for instantiation.
- **Reset once per request.** `StencilRenderer` is `scoped` (new per request) and calls
  `resetSsrDocData` once, before the request's first fragment, so all fragments on a page share
  one hydration ID sequence. Full-document renders don't need it.
- **On `__error`** (or an FFI exception), the renderer logs it and returns the un-rendered HTML.

## Things to know

- **`ffi.enable=true` must be in php.ini.** The default (`preload`) only allows FFI on the CLI,
  so FrankenPHP, PHP-FPM and `artisan serve` fail. A `-d` flag isn't passed to `artisan serve`'s
  child process.
- **Octane's auto-downloaded FrankenPHP binary may not have FFI.** Check with
  `frankenphp php-cli -m`.
- **`libextism` location is fixed.** The SDK only looks in `/usr/local/lib` and `/usr/lib`.
- **Don't use a global `$lib`.** The SDK stores its FFI handle in a global with that name.
- **Errors come back two ways.** Plugin errors are `{"__error": …}` JSON in the output; traps
  (bad function name, wasm trap) throw `Extism\FunctionCallException`.
- **Use a Blade directive, not a component.** A `<x-stencil>` component returning a string would
  have the rendered HTML compiled as Blade again.
- **Without Octane it works, but slowly.** Under `artisan serve` or PHP-FPM the plugin is
  instantiated on every request (~150 ms with a warm cache, 6–10 s cold).

## Timings

Rough numbers from a small Docker VM (1 CPU); see [Render times](../../README.md#render-times).

| What | Time |
|---|---|
| Compile plugin (wasmtime cache cold, done once at `docker build`) | 5.8–9.9 s |
| Instantiate plugin (cache warm; once per Octane worker) | ~160 ms |
| `GET /health`, Laravel baseline with no rendering (Octane) | 5 ms |
| `GET /`, Octane (1 whole-document `renderToString`) | ~46 ms |
| `GET /helper`, Octane (4 fragment `renderToString` calls) | ~59 ms |
| `GET /` without Octane (`php artisan serve`, plugin instantiated per request) | 147 ms |
| `GET /helper` without Octane (plugin instantiated per request) | ~360 ms |

Memory: the container uses ~160 MiB with 2 workers, each holding a plugin.
