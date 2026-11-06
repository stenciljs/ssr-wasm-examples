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
        // Helper mode: server-render the enclosed component markup as a fragment.
        //
        //   @stencil
        //     <demo-task-list tasks="{{ json_encode($tasks) }}"></demo-task-list>
        //   @endstencil
        //
        // The content is ordinary Blade (escaped as usual); its output is buffered and passed
        // through renderToString with {"fullDocument": false}.
        Blade::directive('stencil', fn () => '<?php ob_start(); ?>');
        Blade::directive('endstencil', fn () => '<?php echo app(\App\Ssr\StencilRenderer::class)->fragment(ob_get_clean()); ?>');
    }
}
