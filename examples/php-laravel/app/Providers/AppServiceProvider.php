<?php

namespace App\Providers;

use App\Ssr\SsrPlugin;
use App\Ssr\StencilRenderer;
use Illuminate\Support\Facades\Blade;
use Illuminate\Support\ServiceProvider;

class AppServiceProvider extends ServiceProvider
{
    public function register(): void
    {
        // One plugin per worker process (Octane keeps singletons between requests).
        $this->app->singleton(SsrPlugin::class, fn () => new SsrPlugin(config('ssr.wasm_path')));

        // One renderer per request (it resets fragment hydration ids once, before the first fragment).
        $this->app->scoped(StencilRenderer::class);
    }

    public function boot(): void
    {
        // Helper mode: `@stencil ... @endstencil` buffers the enclosed Blade output and renders
        // it as a fragment. Directives, not a Blade component, so the result isn't compiled again.
        //
        //   @stencil<demo-task-list tasks="{{ json_encode($tasks) }}"></demo-task-list>@endstencil
        Blade::directive('stencil', fn () => '<?php ob_start(); ?>');
        Blade::directive('endstencil', fn () => '<?php echo app(\App\Ssr\StencilRenderer::class)->fragment(ob_get_clean()); ?>');
    }
}
