# Go + `net/http`

Server-renders the demo components from Go using only the standard library (`net/http`,
`html/template`) and the pure-Go [Extism SDK](https://github.com/extism/go-sdk), which runs the
plugin on [wazero](https://wazero.io). No cgo, so the result is one static binary. Port 8004.

## Run it

### Docker

From the repo root (run `pnpm build` first):

```sh
docker build -f examples/go-nethttp/Dockerfile -t ssr-wasm-go-nethttp .
docker run --rm -p 8004:8004 ssr-wasm-go-nethttp
open http://localhost:8004
```

Behind a TLS-inspecting proxy, `go mod download` fails with `x509: certificate signed by unknown
authority`. Pass the proxy's root CA as a build secret (used only for the download, not stored in
the image), or set `EXTRA_CA_CERT` when using `scripts/docker-build.sh`:

```sh
docker build --secret id=ca,src=/path/to/proxy-root-ca.pem -f examples/go-nethttp/Dockerfile -t ssr-wasm-go-nethttp .
```

Unit tests need the plugin, so they run in the build stage:

```sh
docker build -f examples/go-nethttp/Dockerfile --target test .
```

### Natively

Requires Go 1.27+. From this directory (default paths point at `packages/components/dist`):

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

Conformance tests:

```sh
cd tests/conformance && BASE_URL=http://localhost:8004 npx playwright test
```

## Where things are

| File | What it does |
|---|---|
| [`internal/ssr/ssr.go`](internal/ssr/ssr.go) | Plugin wrapper: compile once, instance pool, per-request handle |
| [`middleware.go`](middleware.go) | `ssrMiddleware`: buffers `text/html` responses on `/` and renders the whole document |
| [`pages.go`](pages.go) | `stencil` template func for `/helper` (`newHelperTemplates`) |
| [`templates/components.html`](templates/components.html) | The components as named templates, used by the helper |
| [`templates/index.html`](templates/index.html), [`templates/helper.html`](templates/helper.html) | The two pages |
| [`main.go`](main.go) | Routes, `/health`, and `/stencil/*` (client bundle via `http.FileServer`) |

In a template, `{{stencil .SSR "demo-card" .}}` executes the named template, renders the result
as a fragment and returns `template.HTML`. `.SSR` is the request's handle, so all fragments on a
page share one instance.

## How it uses the plugin

```go
renderer, _ := ssr.New(ctx, ssr.Config{WasmPath: "...", PoolSize: 4})

h, err := renderer.Acquire(r.Context()) // take an instance
defer h.Release()                        // give it back
html := h.Render(page, nil)              // whole document
frag := h.Render(markup, ssr.Fragment)   // {"fullDocument": false}
```

- **Compile once:** `extism.NewCompiledPlugin` compiles the module once per process.
- **Pool:** `SSR_POOL_SIZE` instances in a buffered channel, since an instance isn't safe for
  concurrent use. `Acquire` waits for a free one or gives up when the request's context is done.
- **Hydration IDs:** the handle calls `resetSsrDocData` once per request, before its first
  fragment. Full-document renders don't need it.
- **Errors:** on `{"__error": ...}` or a failed call, `Render` logs it and returns the input HTML
  unchanged; the components still render on the client.
- Both routes send `Server-Timing: ssr;dur=…` with the time spent in the plugin.

## Things to know

- **Cancellation closes the instance.** Every call runs under the request's context plus a 30 s
  timeout. When wazero aborts a call it also closes the instance, so the handle is marked broken
  and `Release` replaces it in the background (logged as a warning).
- **Pool size:** a render is pure CPU work, so a pool larger than the CPU count only queues. The
  default `GOMAXPROCS` follows the container's CPU limit (Go 1.25+).
- **Compilation cache:** `SSR_CACHE_DIR` cuts startup from ~1-2 s to ~100 ms. The directory must
  be writable by the server's user, or compiling fails. A stale cache (different wazero version,
  module or CPU) is just a miss.
- **Clocks:** instances use real clocks (`WithSysWalltime`/`WithSysNanotime`) instead of wazero's
  deterministic fakes. Component `console.*` output goes to the server's stderr.
- **Image:** multi-stage, ending in `distroless/static:nonroot` with a pre-filled compilation
  cache.

## Timings

Rough numbers from a small Docker VM (1 CPU); see [Render times](../../README.md#render-times).
Renders on wazero are noticeably slower than in the examples that run on wasmtime.

| | |
|---|---|
| Compile (`NewCompiledPlugin`), no cache | 0.8-2.0 s |
| Compile, wazero cache warm | 80-160 ms |
| Instantiate one instance, including its first call | 3-8 ms |
| Render `/` (middleware, whole document) | ~133 ms |
| Render `/helper` (reset + 4 fragments) | ~162 ms |
| `BenchmarkRenderFragment` (`demo-card` + badge) | ~55-75 ms |
| Memory, idle, pool of 1 / pool of 4 | ~45 MB / ~46 MB RSS |
| Image size | ~29 MB |
