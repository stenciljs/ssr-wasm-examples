<?php

namespace App\Ssr;

use Illuminate\Support\Facades\Log;

/**
 * Renders Stencil components for the current request. Used by both the middleware and the
 * `@stencil` Blade directive.
 *
 * Bound as `scoped`, so there's a fresh instance per request. Before the request's first
 * fragment it resets the plugin's hydration ids, so all fragments on a page share one sequence.
 * Full-document renders don't need the reset.
 */
class StencilRenderer
{
    private bool $fragmentIdsReset = false;

    public function __construct(private readonly SsrPlugin $plugin) {}

    /**
     * Server-render the Stencil components in `$html`.
     *
     * On failure, logs the error and returns `$html` unchanged; the components then render in
     * the browser.
     *
     * @param  array<string, mixed>  $options  e.g. `['fullDocument' => false]` for a fragment
     */
    public function render(string $html, array $options = []): string
    {
        try {
            if (($options['fullDocument'] ?? true) === false && ! $this->fragmentIdsReset) {
                $this->plugin->resetSsrDocData();
                $this->fragmentIdsReset = true;
            }
            $result = $this->plugin->renderToString($html, $options);
        } catch (\Throwable $e) {
            // A wasm trap throws Extism\FunctionCallException.
            Log::error('[ssr-wasm] renderToString failed', ['exception' => $e]);

            return $html;
        }

        // A plugin error comes back as `{"__error": "..."}`.
        if (isset($result['__error'])) {
            Log::error('[ssr-wasm] '.$result['__error']);

            return $html;
        }

        return $result['html'];
    }

    /** Render a single component (and its light DOM) as a fragment, for templates. */
    public function fragment(string $html): string
    {
        return $this->render($html, ['fullDocument' => false]);
    }
}
