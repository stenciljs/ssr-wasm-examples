# Example contract

Every example in `examples/` serves the **same pages** with the **same content**, so one
conformance suite (`tests/conformance`) can check them all. If you add an example, implement
everything below and then run:

```sh
BASE_URL=http://localhost:<port> pnpm test
```

## Inputs

| What | Where | Served / loaded as |
|---|---|---|
| The Extism plugin | `packages/components/dist/ssr-wasm/index.wasm` | loaded by the server, with **WASI enabled** |
| The client bundle | `packages/components/dist/loader-bundle/demo-components/` | static files under **`/stencil/`** |

Examples read the plugin path from the `SSR_WASM_PATH` env var and the client bundle directory from
`STENCIL_CLIENT_DIR`. Both default to paths relative to the repo, so running from a checkout
works without setting them. The Dockerfiles copy both into the image.

## Plugin lifecycle

- **Compile once, instantiate per worker.** Compiling the plugin (about 6.5 MB of QuickJS and
  bundled components) is expensive; creating instances from the compiled plugin is cheap. An
  instance **isn't thread-safe**: keep one per worker thread or process, or a pool, and never share
  one between concurrent requests.
- **Hydration IDs** (`s-id`, `c-id`, `<!--r.N-->`) only need to be unique within a page.
  - Full-document renders always number them from 1. No reset needed.
  - Fragment renders (`fullDocument: false`) share one sequence in the plugin instance, so the
    fragments of a page never collide. The sequence keeps counting across requests until
    `resetSsrDocData` is called. **Call it once per request, before the first fragment**, so the
    same page always renders the same way (the suite checks this). **Never call it between
    fragments of the same page**, or their IDs will collide.
- `renderToString` takes `{"html": "...", "options": {...}}` and returns JSON. If the result has
  `__error`, log it and fall back to the un-rendered HTML (the components still render on the
  client).

## Routes

### `GET /` - middleware mode

The framework renders the page with its normal templating. Middleware, or the framework's
response hook, then passes the **whole HTML response** through `renderToString` (default options,
`fullDocument: true`). Only `text/html` responses are rendered.

### `GET /helper` - template-helper mode

The page is **not** post-processed. Instead the template calls a helper, filter or tag that renders a single
component's HTML with `{"fullDocument": false}` and inlines the result. Every component
on the page goes through the helper.

### `GET /health`

Returns `200` with body `ok`, without touching the plugin.

### `GET /stencil/*`

The client bundle, served as static files.

## Page content

Both `/` and `/helper` render this markup. `{Framework}` is the example's display name,
e.g. `Django`. `{tasks}` is the JSON-encoded task data below, **escaped by the framework's
template engine** into the attribute (don't hand-escape it):

```html
<!doctype html>
<html lang="en">
  <head>
    <meta charset="utf-8">
    <title>{Framework} · Stencil SSR WASM</title>
    <style>
      body { font-family: system-ui, sans-serif; max-width: 40rem; margin: 2rem auto; }
      main > * { margin-bottom: 1.5rem; }
      .demo-notice { display: block; padding: .75rem 1rem; border-radius: 6px; background: #eef2ff; }
      .demo-notice--error { background: #fee2e2; }
    </style>
    <script type="module" src="/stencil/demo-components.esm.js"></script>
  </head>
  <body>
    <main>
      <h1>{Framework} + Stencil SSR WASM</h1>

      <demo-notice kind="info">Rendered by {Framework}.</demo-notice>

      <demo-card heading="Server-rendered card" variant="highlight">
        <span slot="icon">★</span>
        <p>This card was rendered on the server as Declarative Shadow DOM.</p>
        <demo-badge tone="success">SSR</demo-badge>
        <span slot="footer">Footer slot</span>
      </demo-card>

      <demo-counter start="5" step="2"></demo-counter>

      <demo-task-list tasks="{tasks}"></demo-task-list>
    </main>
  </body>
</html>
```

In **helper mode** each of the four top-level components (`demo-notice`, `demo-card`,
`demo-counter`, `demo-task-list`) is rendered by its own helper call. `demo-badge` is rendered as part of
the `demo-card` call, because it's slotted content.

### Task data

The server builds this data structure in its own language and JSON-encodes it. It deliberately
includes `<`, `>`, `&` and quotes to check escaping:

```json
[
  { "title": "Write components", "done": true },
  { "title": "Render with <wasm> & \"friends\"", "done": false },
  { "title": "Hydrate on the client" }
]
```

## Ports

| Example | Port |
|---|---|
| `tests/conformance` Node implementation of this contract (`pnpm test` starts it) | 8000 |
| `php-laravel` | 8001 |
| `python-django` | 8002 |
| `ruby-rails` | 8003 |
| `go-nethttp` | 8004 |
| `rust-axum` | 8005 |
| `java-spring` | 8006 |
| `dotnet-aspnet` | 8007 |
