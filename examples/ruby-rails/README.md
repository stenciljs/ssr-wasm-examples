# Ruby + Rails

A minimal Rails 8.1 app (Ruby 3.4, Puma) that server-renders Stencil components with the
`ssr-wasm` plugin and the [Extism Ruby SDK](https://github.com/extism/ruby-sdk).

![Ruby / Rails: the demo page, server-rendered by this example](../../docs/screenshots/ruby-rails.png)

The demo page as this example's server sends it, with JavaScript disabled in the browser.

## Run it

With Docker, from the repo root (the build needs `packages/components/dist`):

```sh
docker build -f examples/ruby-rails/Dockerfile -t ssr-wasm-ruby-rails .
docker run --rm -p 8003:8003 ssr-wasm-ruby-rails
open http://localhost:8003/ http://localhost:8003/helper
```

Natively, you need Ruby 3.3+ and libextism: download `libextism-<target>-<version>.tar.gz` from the
[Extism releases](https://github.com/extism/extism/releases) and copy `libextism.so` /
`libextism.dylib` to `/usr/local/lib`. Run `pnpm build` at the repo root first.

```sh
cd examples/ruby-rails
bundle install
bundle exec puma -C config/puma.rb                       # development
RAILS_ENV=production bundle exec puma -C config/puma.rb  # production
```

| Variable | Default | |
|---|---|---|
| `PORT` | 8003 | |
| `RAILS_MAX_THREADS` | 3 | Puma threads and pool size |
| `WEB_CONCURRENCY` | 0 | Puma workers |
| `SSR_WASM_PATH` | `packages/components/dist/...` | The `.wasm` |
| `STENCIL_CLIENT_DIR` | `packages/components/dist/...` | Browser bundle, served at `/stencil/*` |

## How it's wired

See [How it works](../../README.md#how-it-works) for the general steps.

| File | |
|---|---|
| [`lib/stencil_ssr.rb`](lib/stencil_ssr.rb) | The instance pool, `render_document` / `render_fragment` (with the `__error` fallback), and the middleware (`/`), which renders `text/html` responses and returns the instance to the pool. |
| [`lib/stencil_ssr/compiled_plugin.rb`](lib/stencil_ssr/compiled_plugin.rb) | Compiles the plugin once and creates instances from it (see below). |
| [`app/helpers/stencil_helper.rb`](app/helpers/stencil_helper.rb) | Helper (`/helper`): renders the block as a fragment. |

The middleware is registered in [`config/application.rb`](config/application.rb), the pool is
configured in [`config/initializers/stencil_ssr.rb`](config/initializers/stencil_ssr.rb), and
[`config/puma.rb`](config/puma.rb) compiles the plugin at boot, after Puma forks.

- Instances live in a [`connection_pool`](https://github.com/mperham/connection_pool) sized to
  `RAILS_MAX_THREADS`, because an instance can't be shared between threads. A request checks one
  out on its first render and the middleware checks it back in.
- In a template, wrap component markup in the helper:

  ```erb
  <%= stencil do %>
    <demo-counter start="5" step="2"></demo-counter>
  <% end %>
  ```

  The first `stencil` call in a request resets the hydration ids. The middleware skips responses
  that used the helper, so nothing renders twice.
- An instance whose call raised (a trap) is discarded instead of returned to the pool.

For production, also consider timing and metrics around plugin calls, and skipping
`HEAD`, compressed and non-200 responses in the middleware.

## Things to know

- The Ruby SDK has no compile-once API. `Extism::Plugin.new` compiles the module for every
  instance (~7.4 s). [`compiled_plugin.rb`](lib/stencil_ssr/compiled_plugin.rb) binds libextism's
  compile-once functions with FFI and wraps each instance in a normal `Extism::Plugin`.
- Plugin calls hold the GVL, so threads in one process render one at a time. Use Puma workers
  (`WEB_CONCURRENCY=N`) for parallel rendering. Each worker has its own pool, so memory grows
  with workers x threads.
- `Plugin#call` returns an `ASCII-8BIT` string; force it to UTF-8 before parsing.
- `config.middleware.use` puts the middleware after `ActionDispatch::Static` and inside
  `Rack::ETag`, so static files skip it and ETags cover the rendered HTML.
- `tasks="<%= @tasks.to_json %>"` works as is: ERB escapes the attribute, and the component's
  `JSON.parse` undoes Active Support's `<` / `>` / `&` escapes.

## Performance

Small Docker VM (1 CPU), production, one Puma process, 3 threads. See
[Performance](../../README.md#performance).

| | |
|---|---|
| Compile the plugin (once per process, at boot) | ~6.7-7.4 s |
| Instantiate from the compiled plugin (per pool slot) | 5-9 ms |
| `GET /`: one whole-document render | ~45 ms |
| `GET /helper`: four fragment renders | ~57 ms |
| Memory, `WEB_CONCURRENCY=2` after the conformance suite | ~265 MiB in total |
