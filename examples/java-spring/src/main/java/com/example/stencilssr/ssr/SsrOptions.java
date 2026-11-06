package com.example.stencilssr.ssr;

import java.util.Map;

/**
 * Stencil render options (see the {@code ssr} output target docs), sent to the plugin as JSON.
 * Options that are functions, such as {@code beforeSsr}, can't be passed to the plugin.
 */
public final class SsrOptions {
    private SsrOptions() {}

    /** Default options: render a whole document. */
    public static final Map<String, Object> DOCUMENT = Map.of();

    /** Render only the components in the input, not a whole document. */
    public static final Map<String, Object> FRAGMENT = Map.of("fullDocument", false);

    static boolean isFragment(Map<String, Object> options) {
        return Boolean.FALSE.equals(options.get("fullDocument"));
    }
}
