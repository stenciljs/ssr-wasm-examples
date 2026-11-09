# Stencil `ssr-wasm` examples

Server-side rendering of [Stencil](https://stenciljs.com) web components from PHP, Python, Ruby,
Go, Rust, Java and C#.

Stencil's [`ssr-wasm` output target](https://stenciljs.com/docs/ssr-wasm) builds a component
library's server renderer as a WebAssembly [Extism](https://extism.org) plugin. Any server with an
Extism SDK can load it and render the components to HTML, without Node.js. The browser then
hydrates that HTML with the library's normal client bundle.

**Each screenshot is the HTML a different language's server sent, with JavaScript disabled in the
browser.**

| | |
|---|---|
| [![PHP / Laravel](docs/screenshots/php-laravel.png)](examples/php-laravel)<br>PHP / Laravel | [![Python / Django](docs/screenshots/python-django.png)](examples/python-django)<br>Python / Django |
| [![Ruby / Rails](docs/screenshots/ruby-rails.png)](examples/ruby-rails)<br>Ruby / Rails | [![Go / net/http](docs/screenshots/go-nethttp.png)](examples/go-nethttp)<br>Go / `net/http` |
| [![Rust / Axum](docs/screenshots/rust-axum.png)](examples/rust-axum)<br>Rust / Axum | [![Java / Spring Boot](docs/screenshots/java-spring.png)](examples/java-spring)<br>Java / Spring Boot |
| [![C# / ASP.NET Core](docs/screenshots/dotnet-aspnet.png)](examples/dotnet-aspnet)<br>C# / ASP.NET Core | |

## For component authors

Add the `ssr-wasm` output target next to your browser bundle, build, and give server developers
the `.wasm` file and the browser bundle. Most components work unchanged. The main difference is
that object and array props arrive as JSON attributes: declare them with
`@Prop({ serialize: 'json' })`.

[`packages/components`](packages/components) is a working library set up this way, and its README
covers what to ship and how to write components for it.

## For server developers

Pick the example for your stack. Each one is a small app showing both ways to use the plugin,
with a README on how it works and what to watch out for:

| Example | Framework | Extism SDK |
|---|---|---|
| [PHP](examples/php-laravel) | Laravel | `extism/extism` |
| [Python](examples/python-django) | Django | `extism` |
| [Ruby](examples/ruby-rails) | Rails | `extism` gem |
| [Go](examples/go-nethttp) | `net/http` + `html/template` | `extism/go-sdk` |
| [Rust](examples/rust-axum) | Axum + MiniJinja | `extism` crate |
| [Java](examples/java-spring) | Spring Boot + Thymeleaf | `org.extism.sdk:extism` |
| [C#](examples/dotnet-aspnet) | ASP.NET Core | `Extism.Sdk` |

### How it works

1. **Load the plugin once.** Compile `index.wasm` with WASI enabled, once per process. A plugin
   instance isn't thread-safe, so give each worker or thread its own instance, or use a pool.
2. **Render.** Call `renderToString` with `{"html": "...", "options": {...}}`. It returns JSON,
   with the rendered HTML in `html`. If the result has `__error` instead, log it and serve the HTML
   you passed in: the components then render in the browser.
3. **Choose how much HTML to send.** Every example shows both:
   - **Middleware** (`/`): render the whole page as usual, then pass the finished HTML through
     `renderToString`. It's the smallest change to an existing app.
   - **Template helper** (`/helper`): a template tag or helper renders single components with
     `"fullDocument": false`. Only those components go through the plugin.
4. **Reset hydration IDs once per request** when using fragments: call `resetSsrDocData` before the
   request's first fragment, never between fragments of one page. Whole-page renders don't need
   it.
5. **Serve the browser bundle** and load it on the page, so the components hydrate.

### Performance

Median render times on a warm server, in a small Docker VM (1 CPU, ~2.5 GB), `@stencil/core`
5.0.0-beta.35. "Plugin" is time spent in `renderToString`; "end to end" is the whole HTTP
response. Treat them as rough and relative.

| Example | Extism runtime | `/` plugin | `/` end to end | `/helper` plugin | `/helper` end to end |
|---|---|---|---|---|---|
| PHP / Laravel | wasmtime | | 46 ms | | 59 ms |
| Python / Django | wasmtime | 40 ms | 42 ms | 50 ms | 53 ms |
| Ruby / Rails | wasmtime | | 45 ms | | 57 ms |
| Go / `net/http` | wazero | 133 ms | 135 ms | 162 ms | 164 ms |
| Rust / Axum | wasmtime | 36 ms | 38 ms | 42 ms | 44 ms |
| Java / Spring Boot | wasmtime | 47 ms | 58 ms | 56 ms | 71 ms |
| C# / ASP.NET Core | wasmtime | 36 ms | 38 ms | 47 ms | 51 ms |

- **The Extism runtime matters more than the language.** The plugin spends most of its time in
  QuickJS, a JavaScript interpreter. wasmtime runs it several times faster than wazero, which is
  why Go is slower.
- **Node is faster still.** The same page renders in about 9 ms with Stencil's native
  [`ssr`](https://stenciljs.com/docs/ssr) output target in Node, in the same VM (4 ms with
  [`reuseWindow`](https://stenciljs.com/docs/ssr#reusewindow)). If your server is Node already,
  use that instead.
- **40–60 ms per page is still a normal server-rendering budget.** The output can be cached like
  any other template output.

## Running the examples

Requirements: Node 22+, pnpm, Docker, and
[`extism-js`](https://github.com/extism/js-pdk#install-the-compiler) (plus Binaryen's `wasm-merge`
and `wasm-opt`) on your `PATH`.

```sh
pnpm install
pnpm build                                  # builds the component library and its plugin

scripts/docker-build.sh python-django       # build one example's image (or omit the name for all)
docker run --rm -p 8002:8002 ssr-wasm-python-django
open http://localhost:8002
```

The images copy in `packages/components/dist`, so run `pnpm build` first. Each example's README
also covers running it without Docker. Behind a TLS-inspecting proxy, set
`EXTRA_CA_CERT=/path/to/proxy-root-ca.pem` before running `scripts/docker-build.sh`.

## Working on this repo

Every example serves the same pages, described in [CONTRACT.md](CONTRACT.md), and one test suite
in [`tests/conformance`](tests/conformance) checks them all: the server HTML, the page with
JavaScript off, and hydration with JavaScript on.

```sh
pnpm test                                   # against a minimal Node implementation of CONTRACT.md
BASE_URL=http://localhost:8002 pnpm test    # against a running example
scripts/conformance-all.sh                  # every example's image, one after another
```

To add an example, implement CONTRACT.md, give it the next port, add it to
`scripts/conformance-all.sh`, and check it with the suite.
