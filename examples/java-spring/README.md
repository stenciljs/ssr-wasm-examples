# Java + Spring Boot

A Spring Boot 4 app (Spring MVC + Thymeleaf) that server-renders the demo components with the
[Extism Java SDK](https://github.com/extism/java-sdk) (`org.extism.sdk:extism`). The SDK calls
`libextism` (wasmtime) over JNA. Runs on port 8006.

![Java / Spring Boot: the demo page, server-rendered by this example](../../docs/screenshots/java-spring.png)

The demo page as this example's server sends it, with JavaScript disabled in the browser.

## Run it

With Docker, from the repo root:

```sh
docker build -f examples/java-spring/Dockerfile -t ssr-wasm-java-spring .
docker run --rm -p 8006:8006 ssr-wasm-java-spring
```

Open http://localhost:8006. Behind a TLS-inspecting proxy, pass the proxy's root CA as a build secret:

```sh
docker build --secret id=ca,src=/path/to/proxy-root-ca.pem -f examples/java-spring/Dockerfile -t ssr-wasm-java-spring .
```

Natively, you need JDK 25 and `libextism` (`extism lib install` from the
[Extism CLI](https://github.com/extism/cli), or unpack a
[release](https://github.com/extism/extism/releases) into `/usr/local/lib`). From this directory:

```sh
./mvnw spring-boot:run
```

If libextism isn't on the system library path, tell JNA where it is with
`-Dspring-boot.run.jvmArguments=-Djna.library.path=/path/to/lib`.

| Env var | Default |
|---|---|
| `SSR_WASM_PATH` | `../../packages/components/dist/ssr-wasm/index.wasm` |
| `STENCIL_CLIENT_DIR` | `../../packages/components/dist/loader-bundle/demo-components` |
| `SSR_POOL_SIZE` | number of CPUs |

## How it's wired

| File | What it does |
|---|---|
| [`ssr/SsrRenderer.java`](src/main/java/com/example/stencilssr/ssr/SsrRenderer.java) | The plugin: a pool of instances, and `Handle`, one request's instance, with `renderDocument` / `renderFragment` falling back to the input HTML. No Spring dependencies. |
| [`web/SsrFilter.java`](src/main/java/com/example/stencilssr/web/SsrFilter.java) | Middleware (`/`): servlet filter that renders buffered `text/html` responses. |
| [`web/StencilDialect.java`](src/main/java/com/example/stencilssr/web/StencilDialect.java) | Template helper (`/helper`): the Thymeleaf `ssr:render` attribute. |
| [`web/WebConfig.java`](src/main/java/com/example/stencilssr/web/WebConfig.java) | Creates the pool, and serves the browser bundle under `/stencil/`. |

- Instances live in a fair `ArrayBlockingQueue` of `SSR_POOL_SIZE`, because an instance isn't
  thread-safe and more than one per CPU doesn't help. A request borrows one with `acquire()`
  (waiting up to 30 s) and gives it back with `close()`.
- In a template, `ssr:render` names a Thymeleaf fragment. It renders the fragment with the page's
  model, then passes the result to `renderFragment`:
  ```html
  <th:block ssr:render="components :: demo-card"/>
  ```
- The first `ssr:render` in a request acquires a handle and stores it in a request attribute.
  The handle calls `resetSsrDocData` before its first fragment. `SsrFilter` sees the attribute,
  skips the page, and closes the handle when the request ends.
- If a call fails (a trap), the page is served unrendered and `close()` replaces the instance.

## Things to know

- The Java SDK has no compiled-plugin type, so every `new Plugin(...)` compiles the module.
  wasmtime's on-disk cache (`~/.cache/wasmtime`) makes that a one-time cost. It and JNA's
  `~/.cache/JNA` need a writable home directory.
- libextism needs glibc, so it won't run on Alpine.
- Pass `--enable-native-access=ALL-UNNAMED` to silence Java 24+'s warning about JNA calling `System.load`.
- Don't enable virtual threads: a render is a native call that pins the carrier thread, which on
  1 CPU stalls every other request.
- Plugin instances live outside the Java heap, so leave room for them (the image uses
  `-XX:MaxRAMPercentage=40`).

## For production, also consider

- A per-call `timeout_ms` in the manifest, so a runaway render can't hold an instance forever.
- A `Server-Timing` header with the time spent in the plugin.
- Replacing a broken instance in the background instead of on the request that broke it.

## Performance

Rough numbers from a small Docker VM (1 CPU); see [Performance](../../README.md#performance).

| | |
|---|---|
| Render `/` (middleware, whole document) | ~47 ms |
| Render `/helper` (reset + 4 fragments) | ~56 ms |
| Compile the module (first instance, wasmtime cache empty) | ~3.5 s |
| Startup to "Started", with the AOT cache / without | ~3.0-3.4 s / ~5.8-6.6 s |
| Memory, idle, pool of 1 / pool of 4 | ~136 MB / ~200 MB (~150 MB / ~220 MB after some requests) |
