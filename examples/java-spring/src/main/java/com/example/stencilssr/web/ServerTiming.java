package com.example.stencilssr.web;

import java.time.Duration;
import java.util.Locale;

final class ServerTiming {
    private ServerTiming() {}

    /** A {@code Server-Timing} value for the time spent in the plugin. */
    static String ssr(Duration d) {
        return String.format(Locale.ROOT, "ssr;desc=\"Stencil SSR WASM\";dur=%.2f", d.toNanos() / 1e6);
    }
}
