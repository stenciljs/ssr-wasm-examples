<?php

// Inputs from CONTRACT.md. Defaults point into the repo checkout, so `php artisan octane:start`
// from examples/php-laravel works without any env vars.
return [
    'wasm_path' => env('SSR_WASM_PATH', base_path('../../packages/components/dist/ssr-wasm/index.wasm')),
    'client_dir' => env('STENCIL_CLIENT_DIR', base_path('../../packages/components/dist/loader-bundle/demo-components')),
];
