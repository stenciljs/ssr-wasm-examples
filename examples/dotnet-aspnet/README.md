# ASP.NET Core + Stencil SSR WASM

Server-renders Stencil components from C# with ASP.NET Core, Razor Pages and the
[Extism .NET SDK](https://github.com/extism/dotnet-sdk) (`Extism.Sdk`, plus `Extism.runtime.*`
for the native `libextism`, which runs the `ssr-wasm` plugin on wasmtime). No Node.js.

It implements the shared [contract](../../CONTRACT.md) on port **8007**:

| Route | What it shows | Where |
|---|---|---|
| `GET /` | **Middleware mode**: ASP.NET Core middleware buffers the `text/html` response and passes the whole document through `renderToString` | [`Ssr/StencilSsrMiddleware.cs`](Ssr/StencilSsrMiddleware.cs), page in [`Pages/Index.cshtml`](Pages/Index.cshtml) |
| `GET /helper` | **Helper mode**: a `<stencil>…</stencil>` tag helper renders each component as a fragment (`{"fullDocument": false}`); the layout is never sent to the plugin | [`Ssr/StencilTagHelper.cs`](Ssr/StencilTagHelper.cs), page in [`Pages/Helper.cshtml`](Pages/Helper.cshtml) |
| `GET /health` | `ok`, without touching the plugin | [`Program.cs`](Program.cs) |
| `GET /stencil/*` | the client bundle, via `UseStaticFiles` with a `PhysicalFileProvider` on `STENCIL_CLIENT_DIR` | [`Program.cs`](Program.cs) |

The plugin wrapper (compile once, instance pool, per-request handle) is
[`Ssr/SsrRenderer.cs`](Ssr/SsrRenderer.cs) and [`Ssr/SsrHandle.cs`](Ssr/SsrHandle.cs).

## How it works

### The `Ssr` service

```csharp
builder.Services.AddSingleton(new SsrConfig { WasmPath = "...", PoolSize = 4 });
builder.Services.AddSingleton<SsrRenderer>();
builder.Services.AddScoped(sp => sp.GetRequiredService<SsrRenderer>()
    .CreateHandle(sp.GetRequiredService<IHttpContextAccessor>().HttpContext));

// anywhere in a request (middleware, tag helper, page):
var html = await ssr.RenderAsync(page);                       // whole document
var frag = await ssr.RenderAsync(markup, SsrOptions.Fragment); // {"fullDocument": false}
```

- **Compile once.** `SsrRenderer` (a singleton) builds one `Extism.Sdk.CompiledPlugin` with WASI
  enabled, then calls `Instantiate()` on it `SSR_POOL_SIZE` times (default
  `Environment.ProcessorCount`, which respects the container's CPU limit). Each new instance
  calls `resetSsrDocData` once, so QuickJS start-up isn't paid on a request. `Program.cs` resolves
  the renderer before `app.Run()`, so the pool is full before the server listens.
- **A pool of instances, as a bounded `Channel<Plugin>`.** An instance isn't safe for concurrent
  use. Taking one is `await channel.Reader.ReadAsync(RequestAborted)`, giving it back is
  `TryWrite`. A channel gives FIFO waiting, a hard cap on memory, and an *async* wait that takes
  the request's cancellation token, so a queued request doesn't hold a thread. (`ObjectPool<T>`
  doesn't fit: it never blocks, it creates a new object when the pool is empty, which would be
  an unbounded number of instances.)
- **One handle per request (`SsrHandle`, a scoped service).** The middleware and every
  `<stencil>` tag helper in a request get the same handle, so they share one instance. The
  handle takes it from the pool on its first render, so requests that never render (static
  files, `/health`) never touch the pool. The middleware releases it as soon as the endpoint
  has finished, before the response is written, so a slow client can't keep an instance out of
  the pool. Request-scope disposal releases it too, as a backstop.
- **Hydration ids.** Full-document renders (middleware mode) always number their ids from 1, so
  they need no reset. Fragments share one id sequence in the plugin, so the fragments of a page
  never collide. The handle calls `resetSsrDocData` once, before its first fragment, so fragment
  ids also restart at 1 on every request. That's only for repeatable output: without it ids
  would still be unique, and resetting between fragments would make them collide.
- **Errors fall back.** If `renderToString` returns `{"__error": ...}`, or the call fails, the
  handle logs it and returns the HTML it was given; the components still render on the client.
- **Cancellation and timeouts.** Every call gets the request's `RequestAborted` token (the SDK
  turns it into `extism_plugin_cancel`) and the manifest has a 30 s `Timeout`. A call that
  fails for any reason marks the handle broken, and on release the instance is disposed and
  replaced in the background rather than handed to the next request.

#### Synchronous plugin calls and the thread pool

`Plugin.Call` is synchronous: it runs the wasm on the calling thread until the render is done.
The handle calls it inline, on the request's thread-pool thread, and doesn't wrap it in
`Task.Run`:

- A render is pure CPU work (~40-60 ms here). There's no I/O to overlap, so `Task.Run` would
  only move the same work to another thread-pool thread and add a hop.
- The pool size bounds it. At most `SSR_POOL_SIZE` threads can be inside the plugin at once,
  and that defaults to the CPU count, the same number of threads the thread pool keeps ready.
  So renders can't starve the thread pool. Even with every instance busy, there are threads
  left for other requests (`/health`, static files), and new threads get injected if needed.
- Requests waiting for an instance wait asynchronously (`ReadAsync`), holding no thread.

If you raise `SSR_POOL_SIZE` well above the core count, renders queue on the CPU instead, and
long renders can tie up enough threads to make the thread pool inject more. That's another
reason to keep the pool at the core count.

### Middleware mode

`StencilSsrMiddleware` runs after `UseRouting`, for every request that reaches it (static files
are served earlier in the pipeline). It swaps in an `IHttpResponseBodyFeature` whose stream
decides on the first write, when the endpoint has set its headers, what to do with the response:

- **`text/html` (UTF-8, not already encoded, has a body): buffered.** When the endpoint returns,
  the middleware passes the whole document through `renderToString` with default options, sets
  `Content-Length` and writes the result.
- **Anything else: passed straight through,** without touching the plugin.
- **Except pages that used the helper.** A whole-page render of a page that already contains
  rendered fragments would render those components again. The handle records when it rendered a
  fragment, and the middleware sends those pages through unchanged. That's how `/helper` is left
  alone. So every HTML page is server-rendered by default, and a page opts out by using
  `<stencil>`.

### Helper mode

The four components are ordinary Razor partials ([`Pages/Shared/_Demo*.cshtml`](Pages/Shared)),
shared with middleware mode. Razor does all the escaping, including the task JSON in
`<demo-task-list tasks="@Model.TasksJson">`. The JSON is a plain `string`, so Razor
attribute-encodes it. Nothing is escaped by hand, and `System.Text.Json`'s own `<`-style
escaping is switched off (`UnsafeRelaxedJsonEscaping`), because HTML escaping is Razor's job. The
page wraps each partial:

```cshtml
<stencil><partial name="_DemoCard" /></stencil>
```

`StencilTagHelper` runs its child content (the `<partial>`), renders the markup with the
request's handle and `SsrOptions.Fragment`, and replaces itself with the result. The tag helper
gets the scoped `SsrHandle` from its constructor, so all four share one instance and one id
sequence.

Both modes send a `Server-Timing: ssr;dur=…` header with the time spent in the plugin.

## Run it

### Docker

The build context is the repo root:

```sh
docker build -f examples/dotnet-aspnet/Dockerfile -t ssr-wasm-dotnet-aspnet .
docker run --rm -p 8007:8007 ssr-wasm-dotnet-aspnet
```

Behind a TLS-inspecting proxy, `dotnet restore` fails with `The SSL connection could not be
established` / `UntrustedRoot`. Pass the proxy's root CA as a build secret. It's only used for
the restore (through `SSL_CERT_FILE`) and isn't stored in the image:

```sh
docker build --secret id=ca,src=/path/to/proxy-root-ca.pem -f examples/dotnet-aspnet/Dockerfile -t ssr-wasm-dotnet-aspnet .
```

The image is a multi-stage build. `sdk:10.0-noble` restores packages in a layer of their own,
then publishes framework-dependent for `linux-x64`. The result runs on
`aspnet:10.0-noble-chiseled` (Ubuntu chiseled: no shell or package manager, non-root `app`
user). The image holds the app, `libextism.so` (~27 MB), the plugin, the client bundle and a
pre-filled wasmtime cache, about 162 MB in all (124 MB of that is the base image).
[`Dockerfile.dockerignore`](Dockerfile.dockerignore) keeps the build context down to the files
it copies. The `HEALTHCHECK` runs `dotnet server.dll --healthcheck`, because there's no curl.

Unit and integration tests (xUnit v3 + `WebApplicationFactory`; they need the plugin, so they
run in the build stage):

```sh
docker build -f examples/dotnet-aspnet/Dockerfile --target test .
```

### Natively

With the .NET 10 SDK, from this directory (the default paths point at `packages/components/dist`).
The native runtime package defaults to the SDK's own platform (`Extism.runtime.osx-arm64` on an
Apple Silicon Mac, for example). Override it with `-p:ExtismRuntime=<rid>`.

```sh
dotnet run
dotnet test tests
```

| Env var | Default |
|---|---|
| `ASPNETCORE_HTTP_PORTS` / `ASPNETCORE_URLS` | unset: `http://localhost:8007`; `8007` in the image |
| `SSR_WASM_PATH` | `../../packages/components/dist/ssr-wasm/index.wasm` |
| `STENCIL_CLIENT_DIR` | `../../packages/components/dist/loader-bundle/demo-components` |
| `SSR_POOL_SIZE` | `Environment.ProcessorCount` |

Then check it against the conformance suite:

```sh
cd tests/conformance && BASE_URL=http://localhost:8007 npx playwright test
```

## Timings

Rough numbers from a small Docker VM (1 CPU, ~2.5 GB). Render times are medians of 30 sequential
requests on a warm server, measured the same way for every example (see
[Render times](../../README.md#render-times) in the root README). Startup numbers come from the
server's logs.

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

wasmtime (Cranelift) runs QuickJS a few times faster than the Go example's wazero, so renders are
much faster than the Go example's. The cost moves to compilation: Cranelift takes
seconds to compile the ~6.5 MB module on one CPU, which is why the image compiles it once at
build time (below).

### The wasmtime cache

`libextism` turns on wasmtime's compilation cache, under `$HOME/.cache/wasmtime`. The Dockerfile
runs `dotnet server.dll --warm-cache` (compile, instantiate once, exit) as the runtime user, so the
image ships a warm cache (~4 MB) and the server starts compiling in ~100 ms instead of seconds. The
cache key includes the CPU features, so on a different CPU it's just a miss: the server compiles as
usual and writes the cache. The runtime user needs a writable home for that. `HOME=/home/app` is
set explicitly, because a home that isn't writable makes compilation fail.

## Notes

- **Pool size vs. CPUs.** Instances are cheap to create, so it's tempting to make the pool
  large. But a render is pure CPU, so extra instances just queue on the same cores (see
  [the thread-pool notes above](#synchronous-plugin-calls-and-the-thread-pool)).
- **`Plugin.Call` returns a span into the plugin's memory.** The `ReadOnlySpan<byte>` overloads
  return the output without copying it, but the span is only valid until the next call on that
  instance. `SsrHandle` parses it immediately. The `string` overloads copy, if you'd rather not
  think about it.
- **Cancellation reports as a timeout.** A call cancelled through the `CancellationToken` throws
  `ExtismException: timeout. Exit Code: 134`, the same as a manifest timeout. The handle checks
  `RequestAborted` to tell the two apart in the logs.
- **The runtime package isn't chosen automatically.** `Extism.Sdk` has only the P/Invoke
  bindings. You add `Extism.runtime.<rid>` (or `Extism.runtime.all`, which pulls in the native library
  for every platform) yourself. This project picks the one for the SDK's platform, and the
  Docker build pins `linux-x64`.
- **Data protection keys.** Razor Pages registers antiforgery, which creates a data-protection
  key ring at start-up and warns that it's stored in the container. This app has no forms or
  cookies, so [`appsettings.json`](appsettings.json) silences that logger. A real app would
  persist the keys.
