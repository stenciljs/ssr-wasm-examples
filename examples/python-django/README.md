# Python + Django

A Django app that server-renders the demo components by loading the `ssr-wasm` plugin with the
[Extism Python SDK](https://pypi.org/project/extism/). Runs on port 8002.

## Run it

### Docker

From the repo root (run `pnpm build` first):

```sh
docker build -f examples/python-django/Dockerfile -t ssr-wasm-python-django .
docker run --rm -p 8002:8002 ssr-wasm-python-django
```

Open http://localhost:8002.

### Natively (Python 3.12+)

Build the components first, so `packages/components/dist` exists. The default paths point there.

```sh
cd examples/python-django
python -m venv .venv && . .venv/bin/activate      # or: uv venv && uv pip install -r requirements.txt
pip install -r requirements.txt
gunicorn                                         # reads gunicorn.conf.py; http://localhost:8002
```

Then, from `tests/conformance`: `BASE_URL=http://localhost:8002 pnpm test`.

### Configuration

| Variable | Default | |
|---|---|---|
| `SSR_WASM_PATH` | `packages/components/dist/ssr-wasm/index.wasm` | the plugin |
| `STENCIL_CLIENT_DIR` | `packages/components/dist/loader-bundle/demo-components/` | client bundle, served at `/stencil/` |
| `WEB_CONCURRENCY` | 2 | gunicorn worker processes |
| `GUNICORN_THREADS` | 4 | threads per worker |
| `PORT` | 8002 | |
| `SSR_LOG_LEVEL` | `INFO` | log level for `stencil_ssr` |

## Where things are

| Path | What it does |
|---|---|
| `stencil_ssr/ssr.py` | Plugin wrapper: compile, per-thread instances, render, error fallback |
| `stencil_ssr/middleware.py` | `StencilSSRMiddleware`, renders the whole response on `/` |
| `stencil_ssr/templatetags/stencil.py` | `{% stencil %}…{% endstencil %}` block tag, renders fragments on `/helper` |
| `demo_site/settings.py` | No database, the two middlewares, plugin and bundle paths from env |
| `demo_site/urls.py`, `demo_site/views.py` | Routes and task data |
| `demo_site/templates/` | `base.html`, `middleware.html`, `helper.html`, `components/*.html` |
| `gunicorn.conf.py` | `gthread` workers; compiles the plugin in `post_worker_init` |

The client bundle is served by WhiteNoise, so it works with `DEBUG` off.

## How it uses the plugin

- **Compile once per worker.** `CompiledPlugin` is created in gunicorn's `post_worker_init`, after
  the fork (`preload_app` is off), so wasmtime's threads and memory mappings are never forked.
- **One instance per thread** (`threading.local`), created lazily from the compiled module (a few
  ms). With `gthread`, a request stays on one thread start to finish, so this is as safe as a pool
  and caps instances at `workers × threads`.
- **`resetSsrDocData`** is called once per request, before the first `{% stencil %}` block, via a
  handle from `for_request(request)` cached on the request. Full-document renders on `/` don't
  need it.
- **On `__error`** (or a result with no `html`), the error is logged and the un-rendered HTML is
  served; the components still render on the client. If a call traps (`extism.Error`), the thread
  drops its instance and creates a new one on its next request.

## Things to know

- **The cache dir must be writable.** Extism keeps wasmtime's compilation cache under
  `$HOME/.cache`, so the app must run as a user with a writable home. As `nobody`, compilation
  fails with `Permission denied`. The Dockerfile compiles the plugin once at build time to warm
  this cache.
- **Sync views only.** Per-thread instances rely on a request never changing threads. Async views
  or streaming responses would need a pool instead.
- **Scale with processes.** The SDK releases the GIL during a call, so threads can render in
  parallel on multiple cores, but use `WEB_CONCURRENCY` to scale further.
- **The middleware skips `/helper` automatically.** The `{% stencil %}` tag marks the request, and
  the middleware skips those responses so already-rendered markup isn't rendered again. It also
  skips streaming, non-HTML and compressed responses. Rendered responses get a
  `Server-Timing: ssr;dur=<ms>` header.
- **Complex props:** the view passes `json.dumps(tasks)` to `tasks="{{ tasks_json }}"` and
  Django's autoescaping handles the quoting. Nothing is escaped by hand.

## Timings

Rough numbers from a small Docker VM (1 CPU); see [Render times](../../README.md#render-times).
Render times are from `Server-Timing`, so they include Python's JSON encoding.

| | |
|---|---|
| Compile module (`CompiledPlugin`), once per worker | several seconds cold, 0.3–1.1 s with the cache warmed at build time |
| Instantiate plugin, once per thread | 0.4–50 ms |
| Render `/` (one full-document call) | ~40 ms (~42 ms end to end) |
| Render `/helper` (four fragment calls) | ~50 ms (~53 ms end to end) |
