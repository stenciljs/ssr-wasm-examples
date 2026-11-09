package com.example.stencilssr.ssr;

import java.nio.file.Path;
import java.util.List;
import java.util.Map;
import java.util.concurrent.ArrayBlockingQueue;
import java.util.concurrent.BlockingQueue;
import java.util.concurrent.TimeUnit;

import org.extism.sdk.Plugin;
import org.slf4j.Logger;
import org.slf4j.LoggerFactory;

import tools.jackson.databind.JsonNode;
import tools.jackson.databind.json.JsonMapper;

/**
 * The Stencil {@code ssr-wasm} Extism plugin, as a fixed pool of instances. An instance isn't
 * thread-safe, so a request takes one for its whole lifetime:
 *
 * <pre>{@code
 * try (SsrRenderer.Handle h = renderer.acquire()) {
 *     String page = h.renderDocument(html);   // whole document
 *     String frag = h.renderFragment(markup); // {"fullDocument": false}
 * }
 * }</pre>
 *
 * <p>The Java SDK has no compiled-plugin type, so every {@code new Plugin(...)} compiles the
 * module with wasmtime. wasmtime's on-disk cache ({@code ~/.cache/wasmtime}) makes that a
 * one-time cost: later instances load the cached machine code.
 */
public final class SsrRenderer implements AutoCloseable {

    private static final Logger log = LoggerFactory.getLogger(SsrRenderer.class);
    private static final JsonMapper JSON = JsonMapper.builder().build();

    private final byte[] manifest;
    private final BlockingQueue<Plugin> pool;

    public SsrRenderer(Path wasmPath, int poolSize) {
        this.manifest = JSON.writeValueAsBytes(
                Map.of("wasm", List.of(Map.of("path", wasmPath.toAbsolutePath().toString()))));
        this.pool = new ArrayBlockingQueue<>(poolSize, true); // fair: first come, first served
        for (int i = 0; i < poolSize; i++) {
            pool.add(newInstance());
        }
        log.info("ssr-wasm plugin ready: {} instance(s) of {}", poolSize, wasmPath);
    }

    private Plugin newInstance() {
        Plugin p = new Plugin(manifest, true, null); // true: WASI on (the plugin needs it)
        // The first call starts QuickJS and loads the components; pay for it here, not on a request.
        p.call("resetSsrDocData", new byte[0]);
        return p;
    }

    /**
     * Takes an instance out of the pool, waiting up to 30 s. If none comes free, the handle
     * renders nothing (it returns its input), so the page is served unrendered.
     */
    public Handle acquire() {
        Plugin p = null;
        try {
            p = pool.poll(30, TimeUnit.SECONDS);
        } catch (InterruptedException e) {
            Thread.currentThread().interrupt();
        }
        if (p == null) {
            log.error("ssr: no plugin instance free, serving unrendered HTML");
        }
        return new Handle(p);
    }

    /** Closes the idle instances (Spring calls this on shutdown). */
    @Override
    public void close() {
        Plugin p;
        while ((p = pool.poll()) != null) {
            p.close();
        }
    }

    /** One request's exclusive use of an instance. Close it to give the instance back. */
    public final class Handle implements AutoCloseable {

        private Plugin plugin; // null if none was free, or once closed
        private boolean broken;
        private boolean fragmentIdsReset;

        private Handle(Plugin plugin) {
            this.plugin = plugin;
        }

        /** Renders a whole HTML document (default options). */
        public String renderDocument(String html) {
            return render(html, false);
        }

        /**
         * Renders one component's markup with {@code {"fullDocument": false}}. Fragments share
         * the instance's hydration-id sequence, so it's reset once, before this handle's first
         * fragment, and never between fragments of the same page.
         */
        public String renderFragment(String html) {
            return render(html, true);
        }

        /** On any failure, logs and returns {@code html} unchanged; the components still render on the client. */
        private String render(String html, boolean fragment) {
            if (plugin == null || broken) {
                return html;
            }
            try {
                if (fragment && !fragmentIdsReset) {
                    plugin.call("resetSsrDocData", new byte[0]);
                    fragmentIdsReset = true;
                }
                Map<String, Object> options = fragment ? Map.of("fullDocument", false) : Map.of();
                byte[] out = plugin.call("renderToString", JSON.writeValueAsBytes(Map.of("html", html, "options", options)));
                JsonNode result = JSON.readTree(out);
                JsonNode error = result.get("__error");
                if (error != null && !error.isNull()) {
                    log.error("ssr: renderToString failed, serving unrendered HTML: {}", error);
                    return html;
                }
                return result.get("html").asString();
            } catch (RuntimeException e) {
                // A trap (ExtismException) can leave the instance unusable: close() replaces it.
                broken = true;
                log.error("ssr: plugin call failed, serving unrendered HTML", e);
                return html;
            }
        }

        /** Returns the instance to the pool, or replaces it with a new one if a call failed. */
        @Override
        public void close() {
            if (plugin == null) {
                return;
            }
            if (!broken) {
                pool.add(plugin);
            } else {
                plugin.close();
                try {
                    pool.add(newInstance());
                } catch (RuntimeException e) {
                    log.error("ssr: replacing a broken plugin instance failed", e);
                }
            }
            plugin = null;
        }
    }
}
