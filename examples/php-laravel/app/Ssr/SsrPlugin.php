<?php

namespace App\Ssr;

use Extism\Manifest;
use Extism\Manifest\PathWasmSource;
use Extism\Plugin;

/**
 * One instance of the Stencil ssr-wasm Extism plugin.
 *
 * Bound as a singleton and warmed by Octane, so each worker loads it once and reuses it. An
 * instance isn't safe for concurrent calls, but a worker handles one request at a time.
 */
class SsrPlugin
{
    private Plugin $plugin;

    public function __construct(private readonly string $wasmPath)
    {
        if (! is_file($wasmPath)) {
            throw new \RuntimeException("ssr-wasm plugin not found at {$wasmPath} (set SSR_WASM_PATH)");
        }

        $this->plugin = $this->load();
    }

    private function load(): Plugin
    {
        // The second argument enables WASI, which the plugin needs.
        return new Plugin(new Manifest(new PathWasmSource($this->wasmPath)), true);
    }

    /** Call a plugin function. If the call throws (e.g. a wasm trap), replace the instance. */
    private function call(string $function, string $input): string
    {
        try {
            return $this->plugin->call($function, $input);
        } catch (\Throwable $e) {
            $this->plugin = $this->load();

            throw $e;
        }
    }

    /** Reset the hydration-id counters. Call once per request, before the first fragment. */
    public function resetSsrDocData(): void
    {
        $this->call('resetSsrDocData', '');
    }

    /**
     * Call `renderToString` (JSON in, JSON out). The decoded result has either `html` or `__error`.
     *
     * @param  array<string, mixed>  $options  Stencil render options, e.g. `['fullDocument' => false]`
     * @return array<string, mixed>
     */
    public function renderToString(string $html, array $options = []): array
    {
        $input = json_encode(['html' => $html, 'options' => (object) $options], JSON_THROW_ON_ERROR);

        return json_decode($this->call('renderToString', $input), true, flags: JSON_THROW_ON_ERROR);
    }
}
