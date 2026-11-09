import type { Config } from '@stencil/core';

export const config: Config = {
  namespace: 'demo-components',
  outputTargets: [
    // Client-side: the lazy-loading bundle. The example servers serve
    // dist/loader-bundle/demo-components/ at /stencil/, and pages load it with
    // <script type="module" src="/stencil/demo-components.esm.js">
    { type: 'loader-bundle' },
    // Server-side: the Extism plugin every example loads.
    { type: 'ssr-wasm' },
  ],
};
