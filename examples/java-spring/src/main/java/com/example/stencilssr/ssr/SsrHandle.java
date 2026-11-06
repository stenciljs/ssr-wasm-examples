package com.example.stencilssr.ssr;

import java.time.Duration;
import java.util.LinkedHashMap;
import java.util.Map;

import org.extism.sdk.Plugin;
import org.slf4j.Logger;
import org.slf4j.LoggerFactory;

import tools.jackson.core.JacksonException;
import tools.jackson.databind.JsonNode;
import tools.jackson.databind.json.JsonMapper;

/**
 * One request's exclusive use of a plugin instance, from {@link SsrRenderer#acquire()}.
 * Not thread-safe; close it (try-with-resources) to give the instance back.
 *
 * <p>Hydration ids: full-document renders always number them from 1. Fragments share one id
 * sequence in the instance, so the fragments of a page never collide, and it keeps counting
 * across requests unless reset. The handle resets it once, before its first fragment and
 * never between fragments, so the same page renders the same way every time.
 */
public final class SsrHandle implements AutoCloseable {

    private static final Logger log = LoggerFactory.getLogger(SsrHandle.class);

    private final SsrRenderer renderer;
    private Plugin plugin; // null once released
    private boolean broken;
    private boolean fragmentIdsReset;
    private long elapsedNanos;

    SsrHandle(SsrRenderer renderer, Plugin plugin) {
        this.renderer = renderer;
        this.plugin = plugin;
    }

    /**
     * Passes {@code html} through {@code renderToString} and returns the rendered HTML. On any
     * failure (a plugin {@code __error} result or a failed call) it logs and returns
     * {@code html} unchanged, so the components still render on the client.
     */
    public String render(String html, Map<String, Object> options) {
        if (plugin == null || broken) {
            return html;
        }
        try {
            if (SsrOptions.isFragment(options) && !fragmentIdsReset) {
                call("resetSsrDocData", new byte[0]);
                fragmentIdsReset = true;
            }
            Map<String, Object> input = new LinkedHashMap<>();
            input.put("html", html);
            input.put("options", options);
            byte[] out = call("renderToString", renderer.json.writeValueAsBytes(input));
            return parseResult(renderer.json, out);
        } catch (RenderException e) {
            log.error("ssr: renderToString failed, serving unrendered HTML: {}", e.getMessage());
        } catch (RuntimeException e) {
            // ExtismException: a trap, or libextism's timeout_ms cancelled the call.
            broken = true;
            log.error("ssr: plugin call failed, serving unrendered HTML", e);
        }
        return html;
    }

    private byte[] call(String function, byte[] input) {
        long start = System.nanoTime();
        try {
            return plugin.call(function, input);
        } finally {
            elapsedNanos += System.nanoTime() - start;
        }
    }

    /** Extracts the HTML from {@code renderToString}'s JSON result, or throws its {@code __error}. */
    static String parseResult(JsonMapper json, byte[] out) {
        JsonNode result;
        try {
            result = json.readTree(out);
        } catch (JacksonException e) {
            throw new RenderException("result isn't JSON: " + e.getOriginalMessage());
        }
        JsonNode error = result.get("__error");
        if (error != null && !error.isNull()) {
            throw new RenderException(error.isString() ? error.asString() : error.toString());
        }
        JsonNode html = result.get("html");
        if (html == null || !html.isString()) {
            throw new RenderException("result has no html");
        }
        return html.asString();
    }

    /** Time spent inside the plugin through this handle (reset + renders). */
    public Duration elapsed() {
        return Duration.ofNanos(elapsedNanos);
    }

    /** Returns the instance to the pool (or has it replaced, if a call failed). Idempotent. */
    @Override
    public void close() {
        if (plugin != null) {
            renderer.release(plugin, broken);
            plugin = null;
        }
    }

    static final class RenderException extends RuntimeException {
        RenderException(String message) {
            super(message);
        }
    }
}
