# Spring Boot + Stencil SSR WASM

Server-renders Stencil components from Java with Spring Boot 4 (Spring MVC + Thymeleaf) and the
[Extism Java SDK](https://github.com/extism/java-sdk), which calls the `ssr-wasm` plugin through
`libextism` (wasmtime) over JNA. No Node.js.

It implements the shared [contract](../../CONTRACT.md) on port **8006**:

| Route | What it shows | Where |
|---|---|---|
| `GET /` | **Middleware mode**: a servlet filter buffers the `text/html` response and passes the whole document through `renderToString` | [`SsrFilter`](src/main/java/com/example/stencilssr/web/SsrFilter.java), page in [`templates/index.html`](src/main/resources/templates/index.html) |
| `GET /helper` | **Helper mode**: a Thymeleaf dialect attribute, `ssr:render`, renders each component as a fragment (`{"fullDocument": false}`); the layout is never sent to the plugin | [`StencilDialect`](src/main/java/com/example/stencilssr/web/StencilDialect.java) + [`SsrRequest`](src/main/java/com/example/stencilssr/web/SsrRequest.java), page in [`templates/helper.html`](src/main/resources/templates/helper.html) |
| `GET /health` | `ok`, without touching the plugin | [`PageController`](src/main/java/com/example/stencilssr/web/PageController.java) |
| `GET /stencil/**` | the client bundle, from `STENCIL_CLIENT_DIR` | [`WebConfig`](src/main/java/com/example/stencilssr/web/WebConfig.java) |

The plugin wrapper (instance pool, per-request handle) is in
[`ssr/`](src/main/java/com/example/stencilssr/ssr/): `SsrRenderer` and `SsrHandle`. It has no
Spring dependencies.

## Which Extism SDK?

Java has two Extism SDKs:

- [`org.extism.sdk:chicory-sdk`](https://github.com/extism/chicory-sdk): pure JVM, runs wasm on
  [Chicory](https://chicory.dev), either interpreted or compiled to JVM bytecode (at runtime, or
  at build time with `chicory-compiler-maven-plugin`). No native library.
- [`org.extism.sdk:extism`](https://github.com/extism/java-sdk): JNA bindings to `libextism`,
  the Rust runtime on wasmtime (a Cranelift JIT). Needs the native library in the image.

Pure JVM would be nicer to deploy, so it was measured first. Same page, same hardware (12-core x86_64,
JDK 17, steady state after 15-60 renders):

| | Instantiate | Render `/` (whole document) | Render one fragment (`demo-card`) |
|---|---|---|---|
| Chicory 1.4 / 1.7.5, interpreter | ~0.7 s | **~9 s** | ~3 s |
| Chicory, runtime compiler (`withAoT()`) | ~3 s per instance | ~550 ms | ~200 ms |
| Chicory, build-time compiler (Maven plugin) | ~0.7 s | ~450-550 ms | ~150-200 ms |
| **libextism 1.41 via JNA (`extism` 1.1.0)** | ~75 ms (cached), ~1.8 s cold | **~33 ms** | **~14 ms** |

Even compiled to bytecode, Chicory is about 15× slower than wasmtime here. The plugin is mostly
QuickJS interpreting the components' JavaScript, and its interpreter loop doesn't turn into
good JVM bytecode (`-XX:-DontCompileHugeMethods` made no difference; neither did Chicory 1.7.5
over the SDK's 1.4.0). On the 1-CPU Docker VM that would be seconds per page. So this example
uses the **libextism SDK**: the Dockerfile downloads `libextism.so` (checksum-verified) and puts
it on `LD_LIBRARY_PATH`.

The Chicory SDK (0.3.0) also needed workarounds that a server example shouldn't have to carry:

- `Plugin.call` rejects exports that return nothing (`The function expects an i32 return
  code`), and extism-js plugins export `() -> ()`. Calling the plugin at all needed reflection.
- `Manifest.Options.withMachineFactory(...)` is silently ignored unless `withAoT()` is also set:
  the build-time-compiled classes are bundled but every call runs in the interpreter.
- The plugin imports Extism's `http_request` host functions, and linking fails with
  `Unresolved symbols` unless you configure an HTTP client or stub them.
- `CompiledPlugin.instantiate()` parses the 6.5 MB module again, and with `withAoT()` recompiles
  it, for every instance.

## How it works

### The `ssr` package

```java
SsrRenderer renderer = new SsrRenderer(new SsrRenderer.Config(wasmPath, poolSize, acquireTimeout, callTimeout));

try (SsrHandle h = renderer.acquire()) {                // take an instance; close() gives it back
    String page = h.render(html, SsrOptions.DOCUMENT);   // whole document
    String frag = h.render(markup, SsrOptions.FRAGMENT); // {"fullDocument": false}
}
```

- **Compile once.** The Java SDK has no compiled-plugin type (libextism has one, but SDK 1.1.0
  doesn't bind it), so each `new Plugin(...)` hands the module to libextism. wasmtime's on-disk
  cache in `~/.cache/wasmtime` is what makes that compile-once: the first instance compiles the
  module (~3.5 s on one CPU) and caches the machine code, and every later instance, in this
  process or the next, loads it (~130-300 ms). The Dockerfile fills the cache at build time.
- **A pool of instances, as an `ArrayBlockingQueue`.** An instance isn't thread-safe, so the
  renderer creates `SSR_POOL_SIZE` instances (default: `availableProcessors()`, which follows the
  container's CPU limit) and puts them in a fair (FIFO) queue. `acquire()` polls it with a timeout
  (30 s), `close()` puts the instance back. Memory is bounded, and a request can't hold more
  than one instance. More instances than CPUs doesn't help: a render is pure CPU work.
- **One handle per request.** Every `render` on a handle uses the same instance.
- **Hydration ids.** Full-document renders (middleware mode) always number their ids from 1, so
  they need no reset. Fragments share one id sequence in the plugin, so the fragments of a page
  never collide. The handle calls `resetSsrDocData` once, before its first fragment, so fragment
  ids also restart at 1 on every request. That's only for repeatable output: without it ids would
  still be unique, and resetting between fragments would make them collide.
- **Timeouts.** The manifest sets `timeout_ms` (30 s), which libextism enforces per call. The SDK's
  `Manifest` class can't express it, so the manifest JSON is written directly. A call that fails
  (trap, timeout) marks the handle broken, and on release the instance is replaced by a fresh
  one on a background thread instead of going back into the pool.
- **Errors fall back.** If `renderToString` returns `{"__error": ...}` or the call fails, `render`
  logs it and returns the HTML it was given; the components still render on the client.
- Each instance is warmed up with a `resetSsrDocData` call when it's created, so no request pays
  for starting QuickJS.

`SsrConfiguration` makes the renderer a singleton bean; Spring closes it on shutdown.

### Middleware mode

[`SsrFilter`](src/main/java/com/example/stencilssr/web/SsrFilter.java) is a
`OncePerRequestFilter` registered for every path except `/stencil/**` and `/health`. It wraps
the response in a `ContentCachingResponseWrapper`, runs the chain, and then, if the response is
`text/html` with a body, no `Content-Encoding` and not a 204/304, acquires a handle, renders the
whole document, and replaces the buffered body. `copyBodyToResponse()` writes the result with
the right `Content-Length`. Anything else is copied through untouched.

The controller and the template ([`index.html`](src/main/resources/templates/index.html)) know
nothing about SSR: the page includes the component fragments with plain `th:replace`.

The filter skips any page whose template used the helper (see below), because a whole-document
render would render those components a second time. So the filter can stay on every route.

### Helper mode

The four components are ordinary Thymeleaf fragments in
[`templates/components.html`](src/main/resources/templates/components.html), so Thymeleaf does
all the escaping, including the task JSON:

```html
<demo-task-list th:fragment="demo-task-list" th:attr="tasks=${tasksJson}"></demo-task-list>
```

`tasksJson` is a plain `String` from Jackson (which doesn't HTML-escape by default), and
`th:attr` escapes it for the attribute: `"` becomes `&quot;`, `<` becomes `&lt;` and so on.
Nothing is escaped by hand. The page ([`helper.html`](src/main/resources/templates/helper.html))
calls:

```html
<th:block ssr:render="components :: demo-task-list"/>
```

`ssr:render` is the one attribute of
[`StencilDialect`](src/main/java/com/example/stencilssr/web/StencilDialect.java), a small Thymeleaf
dialect that Spring Boot registers because it's a bean. Its processor:

1. renders the named fragment with Thymeleaf's own `TemplateManager`, in a fresh context with
   the page's variables, so the component markup is produced, and escaped, exactly as
   `th:replace` would produce it;
2. renders that markup as a fragment with the request's plugin handle;
3. replaces the element with the result, unescaped and not processed again.

Why not `th:utext="${@stencil.fragment(...)}"` with the markup built in Java? Then the bean
would have to build the HTML and escape the JSON itself, which is exactly the hand-escaping the
template engine is there to avoid. With the dialect, the markup lives in a template and the
escaping is Thymeleaf's.

The handle comes from [`SsrRequest`](src/main/java/com/example/stencilssr/web/SsrRequest.java),
a `@RequestScope` bean (the dialect, a singleton, holds its scoped proxy). The first `ssr:render`
of a request acquires an instance; the other three reuse it and its id sequence; Spring
releases it (`@PreDestroy`) when the request completes. Neither the controller nor the template
passes a handle around. If no instance is free in time, the components are inlined unrendered
and render on the client.

Both modes send a `Server-Timing: ssr;dur=…` header with the time spent in the plugin. In helper
mode it's updated after each fragment, which works because Thymeleaf is set to render the whole
page before writing it (`spring.thymeleaf.servlet.produce-partial-output-while-processing=false`).

## Run it

### Docker

The build context is the repo root:

```sh
docker build -f examples/java-spring/Dockerfile -t ssr-wasm-java-spring .
docker run --rm -p 8006:8006 ssr-wasm-java-spring
```

Behind a TLS-inspecting proxy, the Maven and libextism downloads fail with `PKIX path building
failed`. Pass the proxy's root CA as a build secret:

```sh
docker build --secret id=ca,src=/path/to/proxy-root-ca.pem -f examples/java-spring/Dockerfile -t ssr-wasm-java-spring .
```

[`docker/with-proxy-ca.sh`](docker/with-proxy-ca.sh) imports it into a throwaway copy of the
JDK truststore for the duration of each download step, so it isn't stored in any layer. (The JDK
image has no curl, so libextism is fetched by [`docker/Fetch.java`](docker/Fetch.java), which
also checks its SHA-256.)

The image is a multi-stage build:

- **build**: Maven (through the wrapper) on `eclipse-temurin:25-jdk`, with the dependencies
  in their own layer; the jar is extracted into Spring Boot's layers; `jlink` builds a Java
  runtime with only the modules `jdeps` finds (101 MB instead of the 192 MB JRE).
- **runtime**: `ubuntu:noble` (libextism needs glibc), the jlinked runtime, `libextism.so`,
  the app, the plugin and the client bundle. It runs as a non-root user, with
  `-XX:MaxRAMPercentage=40` (the plugin instances live outside the Java heap) and the serial GC.
- A **training run** at build time starts the application context once and exits. It fills the
  JVM's AOT cache (JEP 483/514; startup goes from ~6 s to ~3 s) and wasmtime's compilation cache
  (instances then load in ~150 ms instead of compiling for ~3.5 s).

About 300 MB in all: Ubuntu 78 MB, Java runtime 101 MB, AOT cache 57 MB, libextism 31 MB,
Spring and the other jars 24 MB, plugin 6.5 MB.
[`Dockerfile.dockerignore`](Dockerfile.dockerignore) keeps the build context down to the files it
copies.

Unit tests (they need the plugin and libextism, so they run in the build stage):

```sh
docker build -f examples/java-spring/Dockerfile --target test .
```

### Natively

With JDK 25 and `libextism` installed (`extism lib install` from the
[Extism CLI](https://github.com/extism/cli), or unpack a
[release](https://github.com/extism/extism/releases) into `/usr/local/lib`), from this directory
(the default paths point at `packages/components/dist`):

```sh
./mvnw spring-boot:run
./mvnw test
```

If libextism isn't on the system library path, tell JNA where it is:
`-Dspring-boot.run.jvmArguments=-Djna.library.path=/path/to/lib` for `spring-boot:run`,
`-DargLine=-Djna.library.path=/path/to/lib` for `test`.

| Env var | Default |
|---|---|
| `PORT` | `8006` |
| `SSR_WASM_PATH` | `../../packages/components/dist/ssr-wasm/index.wasm` |
| `STENCIL_CLIENT_DIR` | `../../packages/components/dist/loader-bundle/demo-components` |
| `SSR_POOL_SIZE` | `availableProcessors()` |
| `JAVA_OPTS` (image only) | `-XX:MaxRAMPercentage=40 -XX:+UseSerialGC --enable-native-access=ALL-UNNAMED` |

`STENCIL_CLIENT_DIR` is read at startup, not at build time, so pointing it elsewhere needs no
rebuild. Then check it against the conformance suite:

```sh
cd tests/conformance && BASE_URL=http://localhost:8006 npx playwright test
```

## Timings

Rough numbers from a small Docker VM (1 CPU, ~2.5 GB). Render times are medians of 30 sequential
requests on a warm server, measured the same way for every example (see
[Render times](../../README.md#render-times) in the root README). Startup numbers come from the
server's logs.

| | |
|---|---|
| Startup to "Started", with the AOT cache / without | ~3.0-3.4 s / ~5.8-6.6 s |
| Compile the module (first instance, wasmtime cache empty) | ~3.5 s |
| Instantiate one instance, wasmtime cache warm, incl. first-call init | 130-300 ms |
| Render `/` (middleware, whole document) | ~80 ms |
| Render `/helper` (reset + 4 fragments) | ~80 ms |
| End to end `/` / `/helper` | ~98 ms / ~92 ms |
| First request after startup | ~0.8 s (class loading, JIT) |
| Memory, idle, pool of 1 / pool of 4 | ~136 MB / ~200 MB (~150 MB / ~220 MB after some requests) |

## Notes

- **Instances are cheap to run, slower to create.** Each `new Plugin` goes through libextism's
  compile path and loads the cached code, ~130 ms per instance after the first. That only
  matters at startup and when replacing a broken instance. A compiled-plugin binding in the
  Java SDK would bring it down to milliseconds.
- **Don't turn on virtual threads.** A render is a native call (JNA), which pins the virtual
  thread's carrier for its whole duration; on a 1-CPU container that stalls every other request,
  including `/health`. Platform threads queue on the pool instead, which is what you want.
- **`--enable-native-access=ALL-UNNAMED`** stops Java 24+ warning that JNA calls `System.load`.
  JNA also unpacks its own native stub into `~/.cache/JNA`, so the user needs a writable home.
- **The AOT cache** must be created by the same JVM, with the same classpath, as the one that
  uses it, hence the training run in the final stage and the plain `java -jar app.jar` layout
  (`extract` without `--launcher`). If it doesn't match, the JVM ignores it and starts normally.
- The training run logs some `[aot] class ... cannot be archived` warnings for Spring's
  multi-release classes; they're harmless.
