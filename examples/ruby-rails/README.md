# Ruby on Rails + Stencil SSR WASM

A minimal Rails 8.1 app (Ruby 3.4, Puma, no database or asset pipeline) that server-renders
Stencil components by calling the `ssr-wasm` Extism plugin through the
[Extism Ruby SDK](https://github.com/extism/ruby-sdk). It implements
[`CONTRACT.md`](../../CONTRACT.md) on port **8003**.

| Route | What it shows |
|---|---|
| `GET /` | **Middleware mode.** A plain ERB view; a Rack middleware passes the whole `text/html` response through `renderToString` (`fullDocument: true`). |
| `GET /helper` | **Helper mode.** The layout is left alone; each top-level component is wrapped in `<%= stencil do %> … <% end %>` and rendered as a fragment (`fullDocument: false`). |
| `GET /health` | `ok`, without touching the plugin. |
| `GET /stencil/*` | The client bundle (`STENCIL_CLIENT_DIR`), served by `Rack::Files`. |

## Where things are

| File | |
|---|---|
| [`lib/stencil_ssr/renderer.rb`](lib/stencil_ssr/renderer.rb) | Wraps one plugin instance: `reset!` (`resetSsrDocData`) and `render(html, **options)` (`renderToString`, with the `__error` fallback to the un-rendered HTML + a log line). |
| [`lib/stencil_ssr/session.rb`](lib/stencil_ssr/session.rb) | Per-request: checks an instance out of the pool on first use and renders the document or fragments with it, resetting fragment ids once before the first fragment. |
| [`lib/stencil_ssr/middleware.rb`](lib/stencil_ssr/middleware.rb) | **The middleware.** Renders `text/html` responses and returns the request's instance to the pool. |
| [`lib/stencil_ssr/compiled_plugin.rb`](lib/stencil_ssr/compiled_plugin.rb) | Compile once, instantiate many: binds libextism's compiled-plugin API, which the Ruby SDK doesn't expose (see below). |
| [`lib/stencil_ssr.rb`](lib/stencil_ssr.rb) | Configuration, the per-process compiled plugin, the instance pool and boot-time warm-up. |
| [`app/helpers/stencil_helper.rb`](app/helpers/stencil_helper.rb) | **The view helper** `stencil`. |
| [`app/views/pages/middleware.html.erb`](app/views/pages/middleware.html.erb), [`helper.html.erb`](app/views/pages/helper.html.erb) | The two pages, sharing [`layouts/application.html.erb`](app/views/layouts/application.html.erb). |
| [`config/application.rb`](config/application.rb) | Paths (`SSR_WASM_PATH`, `STENCIL_CLIENT_DIR`) and `config.middleware.use StencilSsr::Middleware`. |
| [`config/routes.rb`](config/routes.rb), [`config/puma.rb`](config/puma.rb) | Routes; threads/workers. |

The task data is a Ruby array in `PagesController`, encoded with `to_json` and escaped into the
attribute by ERB itself: `tasks="<%= @tasks.to_json %>"`. (Active Support additionally writes
`<`, `>` and `&` as `<` etc. inside the JSON; the component's `JSON.parse` turns them back.)

The middleware is added with `config.middleware.use`, which puts it at the end of the stack:
after `ActionDispatch::Static` (static files never reach it) and inside `Rack::ETag` /
`Rack::ConditionalGet` (ETags are computed from the rendered HTML). When a template has used the
`stencil` helper, the middleware leaves that response alone: rendering it again as a whole
document would re-process components that are already rendered.

## Plugin lifecycle

An Extism plugin instance isn't thread-safe, and the hydration-id counter lives inside it, so:

- **The `.wasm` is compiled once per process**, after Puma forks (never in the master). Puma
  warms it up at boot (`after_booted` / `before_worker_boot` in `config/puma.rb`), so no request
  pays for the compile.
- **Instances are created from that compiled plugin and live in a
  [`connection_pool`](https://github.com/mperham/connection_pool)** sized to Puma's thread count
  (`RAILS_MAX_THREADS`, default 3), created on first use. Creating one takes a few milliseconds.
- **A request owns one instance for its whole duration.** The first render in a request checks an
  instance out, and every later render in that request reuses it. The middleware
  checks it back in in an `ensure` once the response body has been built. Requests that render
  nothing (`/health`, `/stencil/*`) never check one out.
- An instance whose call raised (a trap rather than an `__error` result) is discarded instead of
  returned to the pool.
- **Hydration ids.** Full-document renders (middleware mode) always number their ids from 1, so
  they need no reset. Fragments share one id sequence in the plugin, so the four helper fragments
  on `/helper` never collide. The session calls `resetSsrDocData` once, before the request's first
  fragment. That's only for repeatable output: without it ids would still be unique, and
  resetting between fragments would make them collide.

**Why a pool rather than a thread-local instance?** Both give one instance per Puma thread. The
pool makes the ownership explicit and scoped to a request rather than to a thread (it's handed
back at the end of the request, can be discarded when broken, and the number of instances is
capped no matter how many threads touch it), and `connection_pool` drops instances inherited
across `fork`. The cost is the same: at most `RAILS_MAX_THREADS` instances per process.

**Threads vs. workers.** The SDK's FFI calls don't release Ruby's GVL, so while one thread is
inside `renderToString` the others in that process wait. Threads still help with I/O-bound work
around the render, but for parallel rendering on multiple cores use Puma workers
(`WEB_CONCURRENCY=N`): each forked worker builds its own pool and instances after the fork. The
image defaults to `WEB_CONCURRENCY=0` (single process) and 3 threads, which suits a 1-CPU box.
Each instance costs memory (QuickJS heap + the compiled module), so total instances =
workers × threads.

## Run

### Docker (production mode)

From the **repo root** (the build context needs `packages/components/dist`):

```sh
docker build -f examples/ruby-rails/Dockerfile -t ssr-wasm-ruby-rails .
docker run --rm -p 8003:8003 ssr-wasm-ruby-rails
open http://localhost:8003/ http://localhost:8003/helper
```

Environment: `PORT` (8003), `RAILS_MAX_THREADS` (3), `WEB_CONCURRENCY` (0),
`RAILS_LOG_LEVEL` (`info`; `debug` logs every render's timing), `SSR_WASM_PATH`,
`STENCIL_CLIENT_DIR`.

### Natively

Needs Ruby 3.3+ and libextism on the library path (download `libextism-<target>-<version>.tar.gz`
from the [Extism releases](https://github.com/extism/extism/releases) and copy `libextism.so` /
`libextism.dylib` to `/usr/local/lib`). Build the components first (`pnpm build` at the repo
root); the default paths point into `packages/components/dist`.

```sh
cd examples/ruby-rails
bundle install
bundle exec puma -C config/puma.rb                       # development
RAILS_ENV=production bundle exec puma -C config/puma.rb  # production
```

### Conformance

```sh
cd tests/conformance && BASE_URL=http://localhost:8003 npx playwright test
```

## Compile once: an Extism Ruby SDK gap

The `extism` gem (1.0.1) only has `Extism::Plugin.new(wasm, wasi: true)`, which **compiles the
whole module every time**: 7.4 s per instance here. A pool of 3 would pay that three times, and
every instance after a restart would too. libextism has a compile-once API
(`extism_compiled_plugin_new` + `extism_plugin_new_from_compiled`), so
`StencilSsr::CompiledPlugin` binds those two functions with FFI and wraps each new instance in a
normal `Extism::Plugin` (it repeats the SDK's own bookkeeping, so `#call`, `#free` and GC
work unchanged). If a later SDK release adds a compiled-plugin API, use that instead.

Two smaller SDK details: `Plugin#call` returns the output as a binary (`ASCII-8BIT`) string, so
`Renderer#render` forces it to UTF-8 before parsing. And its FFI functions are attached without
`blocking: true`, so a call holds the GVL (see *Threads vs. workers* above).

## Timings

Measured in Docker (production mode, single Puma process, 3 threads) on a 1-CPU / 2.4 GB Docker
VM on an x86_64 Mac, which other image builds were sharing at the time, so treat these as rough:

| | |
|---|---|
| Compile the plugin (once per process, at boot) | ~6.7-7.4 s |
| Instantiate from the compiled plugin (per pool slot) | 5-9 ms |
| `Extism::Plugin.new` instead (compile + instantiate, per instance) | ~7.4 s |
| `GET /`: one whole-document `renderToString` | 125-200 ms (response 145-235 ms) |
| `GET /helper`: four fragment renders | 26-72 ms each, ~170-240 ms total (response 170-450 ms) |
| Memory, `WEB_CONCURRENCY=2` after the conformance suite | ~265 MiB in total |

Start with `RAILS_LOG_LEVEL=debug` to log the time of every `renderToString` call.
