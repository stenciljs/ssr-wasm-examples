# Python + Django

Server-renders Stencil components from a Django app by calling the components' `ssr-wasm`
build (an [Extism](https://extism.org/) WebAssembly plugin) through the
[Extism Python SDK](https://pypi.org/project/extism/). No Node.js runs on the server.

It implements [`CONTRACT.md`](../../CONTRACT.md) on port **8002**:

| Route | What it shows |
|---|---|
| `GET /` | **Middleware mode.** The view renders an ordinary Django template, and a middleware passes the whole `text/html` response through `renderToString` (`fullDocument: true`). |
| `GET /helper` | **Template-tag mode.** Each top-level component is wrapped in a `{% stencil %}…{% endstencil %}` block tag, which renders it as a fragment (`fullDocument: false`). The rest of the layout is left alone. |
| `GET /health` | `ok`, without touching the plugin. |
| `GET /stencil/*` | The client bundle, served by WhiteNoise (works with `DEBUG` off). |

## Where things live

```
examples/python-django/
├── stencil_ssr/                    reusable Django app
│   ├── ssr.py                      plugin wrapper: compile, per-thread instances, render, error fallback
│   ├── middleware.py               StencilSSRMiddleware (middleware mode)
│   └── templatetags/stencil.py     {% stencil %} block tag (helper mode)
├── demo_site/                      the demo project
│   ├── settings.py                 no database, two middlewares, plugin/bundle paths from env
│   ├── urls.py, views.py           the routes and task data
│   └── templates/                  base.html, middleware.html, helper.html, components/*.html
├── gunicorn.conf.py                gthread workers; compiles the plugin in post_worker_init
├── requirements.txt
└── Dockerfile                      build context is the repo root
```

**Which responses the middleware renders.** `StencilSSRMiddleware` renders every complete
`text/html` response, **except** ones whose template already used `{% stencil %}`. The tag
records that on the request, so `/helper` is skipped automatically. Running the whole document
through the plugin again would re-render markup that's already server-rendered. Streaming,
non-HTML and already-compressed responses are skipped too. When the plugin was used, the
response gets a `Server-Timing: ssr;dur=<ms>` header.

**Escaping.** The view JSON-encodes the task list (`json.dumps`) and the template writes
`tasks="{{ tasks_json }}"`. Django's autoescaping turns the quotes, `<`, `>` and `&` into entities.
Nothing is escaped by hand.

## Plugin lifecycle

An Extism plugin instance isn't thread-safe. This example uses:

- **Compile once per process.** `CompiledPlugin` (the parsed and compiled module) is created
  once per gunicorn worker in `post_worker_init`. It isn't created before the fork
  (`preload_app` is off), so wasmtime's threads and memory mappings are never forked.
- **One instance per thread (`threading.local`).** gunicorn's `gthread` worker runs each request
  on a single thread from a fixed-size pool, from start to finish. A thread's instance is
  therefore only ever used by one request at a time, and there are at most
  `workers × threads` instances (default 2 × 4). Instances are created lazily from the compiled
  module, which is cheap (a few ms).
- **Hydration IDs.** Full-document renders (middleware mode) always number their IDs from 1, so
  they need no reset. Fragments share one ID sequence in the plugin, so the fragments of a page
  never collide. `for_request(request)` hands out a per-request handle, cached on the request,
  that calls `resetSsrDocData` once, before the request's first `{% stencil %}` block. That's
  only for repeatable output: without it IDs would still be unique, and resetting between
  fragments would make them collide.
- **Errors.** If the result has `__error`, the error is logged and the un-rendered HTML is
  served. The components still render on the client. The same happens, defensively, if a result
  has no `html`. If a call traps (`extism.Error`), the thread drops that instance and creates
  a fresh one on its next request.

Why threads and not a pool: with `gthread`, request ↔ thread is already a 1:1 binding with a
fixed upper bound, so `threading.local` gives the same isolation as a checkout/return pool
without the queue or the risk of leaking an instance on an exception. The SDK calls into
wasmtime through cffi, which releases the GIL, so renders on different threads can run in
parallel on multi-core hosts. Use processes (`WEB_CONCURRENCY`) to scale further. This design
assumes sync views. Async views or streaming responses would need a pool instead, because a
request could then move between threads.

## Run

### Docker (what the conformance suite runs against)

From the **repo root**:

```sh
docker build -f examples/python-django/Dockerfile -t ssr-wasm-python-django .
docker run --rm -p 8002:8002 ssr-wasm-python-django
```

The image copies `packages/components/dist/ssr-wasm/index.wasm` and
`packages/components/dist/loader-bundle/demo-components/` in and sets `SSR_WASM_PATH` and
`STENCIL_CLIENT_DIR`. It also compiles the plugin once at build time. Extism keeps wasmtime's
compilation cache under `$HOME/.cache`, so this warms the cache for the workers. For the same
reason the app runs as a user that has a home directory: as `nobody`, compilation fails with
`Permission denied`.

Tune with `WEB_CONCURRENCY` (worker processes, default 2), `GUNICORN_THREADS` (default 4),
`PORT` (default 8002) and `SSR_LOG_LEVEL`.

### Natively (Python 3.12+)

Build the components first, so `packages/components/dist` exists. The default paths point there.

```sh
cd examples/python-django
python -m venv .venv && . .venv/bin/activate      # or: uv venv && uv pip install -r requirements.txt
pip install -r requirements.txt
gunicorn                                         # reads gunicorn.conf.py; http://localhost:8002
```

Then, from `tests/conformance`: `BASE_URL=http://localhost:8002 pnpm test`.

## Timings

Measured from the `Server-Timing` header and the app's log lines, over 40 sequential requests
per page. These are wall-clock plugin times, so they include Python's JSON encoding.

| | macOS x86_64, native | Docker (linux/amd64, 1 CPU, load avg ≈ 7–10 from parallel builds) |
|---|---|---|
| Compile module (`CompiledPlugin`), once per worker | ~580 ms cold, ~90–110 ms with warm wasmtime cache | **~16 s cold**, 0.3–1.1 s with the cache warmed at build time |
| Instantiate plugin, once per thread | ~5 ms (+ ~5 ms first-call init) | 0.4–50 ms |
| Render `/` (one full-document call), p50 | 35 ms (p90 39) | 350 ms (min 170) |
| Render `/helper` (four fragment calls), p50 | 50 ms (p90 54) | 700 ms (min 580) |

For comparison, the Node reference server (Extism JS SDK) on the same host takes about 23 ms for
`/` and 34 ms for `/helper` (end-to-end, p50). The Docker numbers mostly show CPU contention on
a shared 1-CPU VM, not the plugin itself.
