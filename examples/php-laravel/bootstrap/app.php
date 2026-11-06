<?php

use Illuminate\Foundation\Application;
use Illuminate\Foundation\Configuration\Exceptions;
use Illuminate\Foundation\Configuration\Middleware;

return Application::configure(basePath: dirname(__DIR__))
    ->withRouting(
        web: __DIR__.'/../routes/web.php',
    )
    ->withMiddleware(function (Middleware $middleware): void {
        // RenderStencilComponents is attached to the `/` route only (routes/web.php), so that
        // `/helper` shows template-helper mode on its own. To server-render every HTML page,
        // append it globally instead: $middleware->append(RenderStencilComponents::class);
    })
    ->withExceptions(function (Exceptions $exceptions): void {
        //
    })->create();
