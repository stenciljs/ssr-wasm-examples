<?php

use App\Ssr\SsrPlugin;
use Laravel\Octane\Octane;

// Only the keys set here override Octane's defaults (vendor/laravel/octane/config/octane.php).
return [
    'server' => env('OCTANE_SERVER', 'frankenphp'),

    // Resolved once when each worker boots and kept for every request it serves: one ssr-wasm
    // plugin per worker. (A singleton first resolved during a request would only last that request.)
    'warm' => [
        ...Octane::defaultServicesToWarm(),
        SsrPlugin::class,
    ],
];
