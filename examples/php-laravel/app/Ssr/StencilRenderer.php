<?php

namespace App\Ssr;

use Illuminate\Support\Facades\Log;

/**
 * Renders Stencil components to HTML for the current request.
 *
 * Bound as `scoped`: Laravel (and Octane) create a fresh instance for each request.
 *
 * Hydration ids: full-document renders always start from 1 on their own. Fragments share one
 * id sequence in the plugin (so the fragments of a page never collide), which keeps counting
 * across requests unless it's reset. That's harmless, but resetting once - before the request's
 * first fragment, never between fragments - gives the same output for the same page.
 */
class StencilRenderer
{
    private bool $fragmentIdsReset = false;

    public function __construct(private readonly SsrPlugin $plugin) {}

    /**
     * Server-render the Stencil components in `$html`.
     *
     * On failure the original HTML is returned unchanged (the components still render on the
     * client), and the error is logged.
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
            Log::error('[ssr-wasm] renderToString failed', ['exception' => $e]);

            return $html;
        }

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
