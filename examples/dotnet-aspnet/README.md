# C# + ASP.NET Core

Server-renders the Stencil components from ASP.NET Core with Razor Pages, using the
[Extism .NET SDK](https://github.com/extism/dotnet-sdk) (`Extism.Sdk` plus an `Extism.runtime.*`
package for the native `libextism`, which runs the plugin on wasmtime). Runs on port 8007.

## Run it

### Docker

Build from the repo root:

```sh
docker build -f examples/dotnet-aspnet/Dockerfile -t ssr-wasm-dotnet-aspnet .
docker run --rm -p 8007:8007 ssr-wasm-dotnet-aspnet
open http://localhost:8007
```

Behind a TLS-inspecting proxy, `dotnet restore` fails with `UntrustedRoot`. Pass the proxy's root
CA as a build secret; it's only used for the restore and isn't stored in the image:

```sh
docker build --secret id=ca,src=/path/to/proxy-root-ca.pem -f examples/dotnet-aspnet/Dockerfile -t ssr-wasm-dotnet-aspnet .
```

Unit and integration tests (xUnit v3 + `WebApplicationFactory`) need the plugin, so they run in the
build stage:

```sh
docker build -f examples/dotnet-aspnet/Dockerfile --target test .
```

### Natively

Requires the .NET 10 SDK. From this directory (default paths point at `packages/components/dist`):

```sh
dotnet run
dotnet test tests
```

Then, from the repo root, run the conformance suite against it:

```sh
cd tests/conformance && BASE_URL=http://localhost:8007 npx playwright test
```

| Env var | Default |
|---|---|
| `ASPNETCORE_HTTP_PORTS` / `ASPNETCORE_URLS` | unset: `http://localhost:8007`; `8007` in the image |
| `SSR_WASM_PATH` | `../../packages/components/dist/ssr-wasm/index.wasm` |
| `STENCIL_CLIENT_DIR` | `../../packages/components/dist/loader-bundle/demo-components` |
| `SSR_POOL_SIZE` | `Environment.ProcessorCount` |

## Where things are

| Path | What it does |
|---|---|
| [`Ssr/SsrRenderer.cs`](Ssr/SsrRenderer.cs) | Plugin wrapper: compiles once, owns the instance pool |
| [`Ssr/SsrHandle.cs`](Ssr/SsrHandle.cs) | Per-request handle: borrows an instance, renders, handles errors |
| [`Ssr/StencilSsrMiddleware.cs`](Ssr/StencilSsrMiddleware.cs) | Middleware for `/`: buffers `text/html` responses and renders the whole document |
| [`Ssr/StencilTagHelper.cs`](Ssr/StencilTagHelper.cs) | `<stencil>…</stencil>` tag helper for `/helper`: renders its content as a fragment |
| [`Pages/Index.cshtml`](Pages/Index.cshtml), [`Pages/Helper.cshtml`](Pages/Helper.cshtml) | The two pages; components are shared partials in [`Pages/Shared`](Pages/Shared) |
| [`Program.cs`](Program.cs) | Service setup, `/health`, `/stencil/*` static files from `STENCIL_CLIENT_DIR` |

## How it uses the plugin

- **Compile once.** `SsrRenderer` (a singleton) builds one `CompiledPlugin` and instantiates it
  `SSR_POOL_SIZE` times before the server listens, calling `resetSsrDocData` on each so QuickJS
  start-up isn't paid on a request.
- **Pooled instances.** Instances aren't safe for concurrent use, so they live in a bounded
  `Channel<Plugin>`: FIFO, capped memory, and an async wait that holds no thread.
- **One handle per request.** `SsrHandle` is a scoped service, so the middleware and every tag
  helper in a request share one instance. It's taken on the first render and returned as soon as
  the endpoint finishes, before the response is written.
- **Hydration ids.** The handle calls `resetSsrDocData` once per request, before the first fragment.
- **Errors.** On `{"__error": ...}` or a failed call, the handle logs it and returns the original
  HTML; the components still render on the client. A failed instance is replaced, not reused.
- **Opting out of middleware.** A page that used `<stencil>` is passed through by the middleware
  unchanged, so components aren't rendered twice.
- Every call gets the request's `RequestAborted` token, and the manifest has a 30 s `Timeout`.
  Responses carry a `Server-Timing: ssr;dur=…` header.

## Things to know

- **Add the native runtime package yourself.** `Extism.Sdk` is only P/Invoke bindings; you need
  `Extism.runtime.<rid>` (or `Extism.runtime.all`). This project picks the SDK's own platform;
  override with `-p:ExtismRuntime=<rid>`. The Docker build pins `linux-x64`.
- **wasmtime needs a writable home.** `libextism` caches compiled code under
  `$HOME/.cache/wasmtime`; an unwritable home makes compilation fail. The image sets
  `HOME=/home/app` and pre-fills the cache at build time (`dotnet server.dll --warm-cache`), so
  compile drops from seconds to ~100 ms. On a different CPU it's a cache miss and compiles as usual.
- **`Plugin.Call` is synchronous.** It runs on the request thread; with the pool capped at the
  core count, renders can't starve the thread pool. Raising `SSR_POOL_SIZE` above the core count
  just queues renders on the CPU.
- **Span results are short-lived.** The `ReadOnlySpan<byte>` overloads point into plugin memory
  and are only valid until the next call on that instance. The `string` overloads copy.
- **Cancellation reports as a timeout.** A cancelled call throws
  `ExtismException: timeout. Exit Code: 134`, same as a manifest timeout. Check `RequestAborted`
  to tell them apart.
- **Chiseled image.** The runtime image has no shell or curl, so `HEALTHCHECK` runs
  `dotnet server.dll --healthcheck`.
- **Data protection warning.** Razor Pages creates a data-protection key ring and warns that it's
  stored in the container. This app has no forms or cookies, so
  [`appsettings.json`](appsettings.json) silences it; a real app would persist the keys.

## Timings

Rough numbers from a small Docker VM (1 CPU); see [Render times](../../README.md#render-times).

| | |
|---|---|
| Compile (`new CompiledPlugin`), cold wasmtime cache | 2.4-7 s |
| Compile, wasmtime cache warm (as in the image) | 95-460 ms |
| Instantiate one instance, including its first call | 5-20 ms |
| Render `/` (middleware, whole document) | ~36 ms (~38 ms end to end) |
| Render `/helper` (reset + 4 fragments) | ~47 ms (~51 ms end to end) |
| `docker run` to first `/health` response | ~2 s |
| Memory, pool of 1: idle / after requests | ~41 MB / ~53 MB |
| Memory, pool of 4: idle / after 16 concurrent requests | ~48 MB / ~68 MB |
| Image size | ~162 MB (124 MB base image) |
