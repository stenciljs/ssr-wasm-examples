# The component library

A small Stencil library, set up the way you'd set up your own to support server-side rendering
from other languages. Every example in this repo renders these components.

## Setup

Two output targets in [`stencil.config.ts`](stencil.config.ts):

```ts
outputTargets: [
  { type: 'loader-bundle' }, // the browser bundle, which hydrates the server's HTML
  { type: 'ssr-wasm' },      // the server renderer, as a WebAssembly plugin
],
```

Building the plugin needs [`extism-js`](https://github.com/extism/js-pdk#install-the-compiler) on
your `PATH`, plus Binaryen's `wasm-merge` and `wasm-opt`. Without it, Stencil writes `index.js`
but skips `index.wasm`, with a warning.

```sh
pnpm build
```

## What to give server developers

| File | What it's for |
|---|---|
| `dist/ssr-wasm/index.wasm` | The plugin. Their server loads it with an [Extism host SDK](https://extism.org/docs/concepts/host-sdk) and calls `renderToString`. |
| `dist/loader-bundle/demo-components/` | The browser bundle. Their pages load `demo-components.esm.js` from it, so the components hydrate. |

Ship both from the same build. The plugin and the browser bundle have to come from the same version
of your components, or hydration fails.

## Writing components that render on the server

The plugin runs your components in QuickJS, a JavaScript interpreter, not in Node or a browser.
Most components need no changes. These are the differences:

- **Complex props have to come in as attributes.** The server only passes HTML to the plugin, so
  an object or array prop arrives as a JSON string. Declare it with
  `@Prop({ serialize: 'json' })` and Stencil parses it for you. See
  [`demo-task-list`](src/components/demo-task-list/demo-task-list.tsx). Check the type before
  using the value: JSON that isn't the shape you expect still parses.
- **Timers run on a virtual clock.** `setTimeout`, `setInterval` and `requestAnimationFrame` work
  during a render, but nothing waits in real time. A timer due after the render's `timeout`
  (default 15 s) never fires, and that render returns an error.
- **No browser APIs.** Layout, `localStorage` and the like aren't there on the server. Guard
  browser-only code with `Build.isBrowser`, as you would for any SSR.
- **Every encapsulation works.** Shadow components are rendered as Declarative Shadow DOM.
  Scoped components get their styles in the page's `<head>`, or at the start of the fragment when
  a server renders a single component.

See the [`ssr-wasm` docs](https://stenciljs.com/docs/ssr-wasm) for the full reference.

## The demo components

| Tag | Encapsulation | Shows |
|---|---|---|
| [`demo-card`](src/components/demo-card/demo-card.tsx) | shadow | named and default slots, `::part`, a reflected prop |
| [`demo-badge`](src/components/demo-badge/demo-badge.tsx) | scoped | scoped styles, used inside both light DOM and a shadow root |
| [`demo-counter`](src/components/demo-counter/demo-counter.tsx) | shadow | state, `componentWillLoad`, an event, interactivity after hydration |
| [`demo-notice`](src/components/demo-notice/demo-notice.tsx) | none | plain light DOM, styled by the page |
| [`demo-task-list`](src/components/demo-task-list/demo-task-list.tsx) | shadow | a JSON array prop, nested components |
