# Go `net/http` + Stencil SSR WASM

Server-renders Stencil components from Go using only the standard library (`net/http`,
`html/template`) and the pure-Go [Extism SDK](https://github.com/extism/go-sdk), which runs the
`ssr-wasm` plugin on [wazero](https://wazero.io). No cgo, no Node.js: the result is one static
binary.

It implements the shared [contract](../../CONTRACT.md) on port **8004**:

| Route | What it shows | Where |
|---|---|---|
| `GET /` | **Middleware mode**: a `func(http.Handler) http.Handler` buffers the `text/html` response and passes the whole document through `renderToString` | [`middleware.go`](middleware.go) (`ssrMiddleware`), page in [`templates/index.html`](templates/index.html) |
| `GET /helper` | **Helper mode**: an `html/template` func renders each component as a fragment (`{"fullDocument": false}`); the layout is never sent to the plugin | `stencil` func in [`pages.go`](pages.go) (`newHelperTemplates`), page in [`templates/helper.html`](templates/helper.html) |
| `GET /health` | `ok`, without touching the plugin | [`main.go`](main.go) |
| `GET /stencil/*` | the client bundle, via `http.FileServer` | [`main.go`](main.go) |

The plugin wrapper (compile once, instance pool, per-request handle) is
[`internal/ssr/ssr.go`](internal/ssr/ssr.go).

## How it works

### The `ssr` package

```go
renderer, _ := ssr.New(ctx, ssr.Config{WasmPath: "...", PoolSize: 4})

h, err := renderer.Acquire(r.Context()) // take an instance
defer h.Release()                        // give it back
html := h.Render(page, nil)              // whole document
frag := h.Render(markup, ssr.Fragment)   // {"fullDocument": false}
```

- **Compile once.** `extism.NewCompiledPlugin` compiles the module to native code once per process.
- **A pool of instances, as a buffered channel.** An instance isn't safe for concurrent use, so
  `New` creates `SSR_POOL_SIZE` instances (default `GOMAXPROCS`) and puts them in a
  `chan *extism.Plugin`. `Acquire` receives from it (or gives up when the request's context is
  done), `Release` sends it back. A channel gives FIFO waiting, context-aware blocking via `select`
  and a hard upper bound on memory, with no locks. More instances than CPUs doesn't help: a render
  is pure CPU work.
- **One handle per request.** Every `Render` on that handle shares the instance.
- **Hydration ids.** Full-document renders (middleware mode) always number their ids from 1, so
  they need no reset. Fragments share one id sequence in the plugin, so the fragments of a page
  never collide. The handle calls `resetSsrDocData` once, before its first fragment, so fragment
  ids also restart at 1 on every request. That's only for repeatable output: without it ids would
  still be unique, and resetting between fragments would make them collide.
- **The handle holds the request's `context.Context`.** Every plugin call runs under it, with a 30 s
  timeout on top (the manifest's `timeout_ms`). wazero aborts a call when its context is cancelled,
  but that also **closes the instance**, so a handle that sees a failed call is marked broken and
  `Release` swaps in a fresh instance in the background instead of pooling a dead one.
- **Errors fall back.** If `renderToString` returns `{"__error": ...}` or the call fails, `Render`
  logs it and returns the HTML it was given; the components still render on the client.
- Instances get real clocks (`WithSysWalltime`/`WithSysNanotime`) rather than wazero's
  deterministic fakes, and the plugin's stdout/stderr (component `console.*`) goes to the server's
  stderr.

### Middleware mode

`ssrMiddleware` wraps only the `/` handler. Its `ResponseWriter` decides when the status is
written: if the `Content-Type` is `text/html` (and the body isn't already encoded) it buffers the
body, otherwise it streams straight through. After the handler returns it acquires a handle,
renders the document, fixes up `Content-Length` and writes it out.

### Helper mode

The four components are ordinary named templates in [`templates/components.html`](templates/components.html),
so `html/template` does all the escaping, including the task JSON in
`<demo-task-list tasks="{{.Tasks}}">` (the JSON is a plain `string`, so it gets attribute-escaped;
nothing is escaped by hand). The page calls:

```gotemplate
{{stencil .SSR "demo-card" .}}
```

`stencil` executes the named template into a buffer (the nested template produces safe markup
first), renders that with the request's handle and `ssr.Fragment`, and returns `template.HTML` so
the rendered markup is inlined unescaped. `.SSR` is the `*ssr.Handle` the handler acquired, so all
four calls share one instance and one id sequence.

Both modes send a `Server-Timing: ssr;dur=…` header with the time spent in the plugin.

## Run it

### Docker

The build context is the repo root:

```sh
docker build -f examples/go-nethttp/Dockerfile -t ssr-wasm-go-nethttp .
docker run --rm -p 8004:8004 ssr-wasm-go-nethttp
```

Behind a TLS-inspecting proxy, `go mod download` fails with `x509: certificate signed by
unknown authority`. Pass the proxy's root CA as a build secret (it's only used for the download and
isn't stored in the image):

```sh
docker build --secret id=ca,src=/path/to/proxy-root-ca.pem -f examples/go-nethttp/Dockerfile -t ssr-wasm-go-nethttp .
```

The image is a multi-stage build ending in `distroless/static:nonroot`: the static binary, the
plugin, the client bundle and a pre-filled wazero compilation cache, about 29 MB in all.
[`Dockerfile.dockerignore`](Dockerfile.dockerignore) keeps the build context down to the files it
copies.

Unit tests (they need the plugin, so they run in the build stage):

```sh
docker build -f examples/go-nethttp/Dockerfile --target test .
```

### Natively

With Go 1.27+, from this directory (the default paths point at `packages/components/dist`):

```sh
go run .
go test ./...
go test -run x -bench . ./internal/ssr
```

| Env var | Default |
|---|---|
| `PORT` | `8004` |
| `SSR_WASM_PATH` | `../../packages/components/dist/ssr-wasm/index.wasm` |
| `STENCIL_CLIENT_DIR` | `../../packages/components/dist/loader-bundle/demo-components` |
| `SSR_POOL_SIZE` | `GOMAXPROCS` |
| `SSR_CACHE_DIR` | unset (no compilation cache); `/app/wazero-cache` in the image |

Then check it against the conformance suite:

```sh
cd tests/conformance && BASE_URL=http://localhost:8004 npx playwright test
```

## Timings

Measured in Docker Desktop on an x86_64 Mac limited to **1 CPU**, with other containers building at
the same time, so treat the numbers as rough. Startup timings come from the server's startup log;
render timings from `Server-Timing` over 20-30 sequential requests.

| | |
|---|---|
| Compile (`NewCompiledPlugin`), no cache | 0.8-2.0 s |
| Compile, wazero cache warm | 80-160 ms |
| Instantiate one instance, including its first call | 3-8 ms |
| Render `/` (middleware, whole document) | ~150-200 ms |
| Render `/helper` (reset + 4 fragments) | ~200-250 ms |
| `BenchmarkRenderFragment` (`demo-card` + badge) | ~55-75 ms |
| Memory, idle, pool of 1 / pool of 4 | ~45 MB / ~46 MB RSS |

Compared with the Node reference server on the same machine (Extism's JS SDK on V8, run on the
host rather than in the 1-CPU VM), which serves `/` in about 30 ms, wazero is noticeably slower
per render. The plugin is mostly QuickJS interpreting the components, so the time is in
interpreting JavaScript inside wasm. Bumping wazero from 1.9.0 (the version the Extism SDK pins)
to 1.12.0 made no difference that showed above the noise, so the example stays on 1.9.0.

### Is the compilation cache worth it?

Compiling the ~6.5 MB module is far cheaper than you might expect, under a second on an idle CPU,
because most of the module is data (the bundled JavaScript), not code. Instances are cheap too
(single-digit milliseconds; extism-js plugins are pre-initialised snapshots). So the cache
doesn't matter for a long-running server. It's still enabled in the image because it costs ~5 MB
and one build step, and cuts startup to ~100 ms, which helps with autoscaling and scale-to-zero.
The cache key includes wazero's version, the module hash and the CPU features, so a mismatch is
just a cache miss: the server compiles as usual and writes the result (the directory is owned by
the runtime user, which matters, because a cache wazero can't write to fails the compile).

## Notes

- **Pool size vs. CPUs.** Since instances are cheap, it's tempting to make the pool large. But a
  render is pure CPU, so extra instances just queue on the same cores. The default is
  `GOMAXPROCS`, which Go 1.25+ derives from the container's CPU limit.
- **Cancellation closes the instance.** This is how wazero implements `WithCloseOnContextDone`
  (which the Extism SDK also turns on whenever the manifest has a timeout). It's handled by
  replacing the instance, as above, and logged as a warning.
- **Helper mode and scoped styles.** A `fullDocument: false` fragment starts with the `<style>`
  elements its scoped components need (here, `demo-badge`'s), so fragments are styled before the
  client loads.
