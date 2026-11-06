import type { Config } from '@stencil/core';

export const config: Config = {
  namespace: 'demo-components',
  outputTargets: [
    // Client-side: lazy-loading bundle the example servers expose at /stencil/.
    // Loaded via <script type="module" src="/stencil/demo-components/demo-components.js">
    { type: 'loader-bundle' },
    // Server-side: the Extism plugin every example loads.
    { type: 'ssr-wasm' },
  ],
};
