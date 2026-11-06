<?php

namespace App\Ssr;

use Extism\Manifest;
use Extism\Manifest\PathWasmSource;
use Extism\Plugin;

/**
 * One instance of the Stencil ssr-wasm Extism plugin.
 *
 * Bound as a singleton, so under Octane it lives as long as the worker: the plugin is
 * created once per worker and reused by every request that worker serves. An Extism plugin
 * isn't safe for concurrent calls, but an Octane/FrankenPHP worker handles one request at a
 * time, so one instance per worker is all the isolation we need.
 */
class SsrPlugin
{
    private Plugin $plugin;

    /** Milliseconds it took to load + instantiate the plugin (reported in a response header). */
    public readonly float $instantiateMs;

    public function __construct(string $wasmPath)
    {
        if (! is_file($wasmPath)) {
            throw new \RuntimeException("ssr-wasm plugin not found at {$wasmPath} (set SSR_WASM_PATH)");
        }

        $start = hrtime(true);
        // QuickJS + the bundled components need WASI.
        $this->plugin = new Plugin(new Manifest(new PathWasmSource($wasmPath)), true);
        $this->instantiateMs = (hrtime(true) - $start) / 1e6;
    }

    /** Reset the hydration-id counters (`s-id`, `c-id`, `<!--r.N-->`). */
    public function resetSsrDocData(): void
    {
        $this->plugin->call('resetSsrDocData', '');
    }

    /**
     * Call `renderToString` and return the decoded result, which has either `html` or `__error`.
     *
     * @param  array<string, mixed>  $options  Stencil render options (JSON-serialisable only)
     * @return array<string, mixed>
     */
    public function renderToString(string $html, array $options = []): array
    {
        $input = json_encode(
            ['html' => $html, 'options' => (object) $options],
            JSON_THROW_ON_ERROR | JSON_UNESCAPED_SLASHES | JSON_UNESCAPED_UNICODE,
        );

        return json_decode($this->plugin->call('renderToString', $input), true, flags: JSON_THROW_ON_ERROR);
    }
}
