<?php

use App\Ssr\SsrPlugin;
use Laravel\Octane\Octane;

// Only the keys set here override Octane's defaults (vendor/laravel/octane/config/octane.php).
return [
    'server' => env('OCTANE_SERVER', 'frankenphp'),

    // Resolved when each worker boots, before it takes requests: every worker instantiates the
    // ssr-wasm plugin once, up front, instead of making its first visitor wait for it.
    'warm' => [
        ...Octane::defaultServicesToWarm(),
        SsrPlugin::class,
    ],
];
