package com.example.stencilssr.ssr;

import static org.assertj.core.api.Assertions.assertThat;
import static org.junit.jupiter.api.Assumptions.assumeTrue;

import java.nio.file.Files;
import java.nio.file.Path;
import java.util.ArrayList;
import java.util.List;
import java.util.regex.Pattern;

import org.junit.jupiter.api.AfterAll;
import org.junit.jupiter.api.BeforeAll;
import org.junit.jupiter.api.Test;

/** Runs against the real plugin (SSR_WASM_PATH, or packages/components/dist) and libextism. */
public class SsrRendererTest {

    static SsrRenderer renderer;

    public static Path wasmPath() {
        String env = System.getenv("SSR_WASM_PATH");
        return Path.of(env != null && !env.isEmpty() ? env : "../../packages/components/dist/ssr-wasm/index.wasm");
    }

    @BeforeAll
    static void setUp() {
        assumeTrue(Files.isReadable(wasmPath()), "plugin not found at " + wasmPath() + " (set SSR_WASM_PATH)");
        renderer = new SsrRenderer(wasmPath(), 2);
    }

    @AfterAll
    static void tearDown() {
        if (renderer != null) renderer.close();
    }

    public static List<String> ids(String html) {
        var m = Pattern.compile("\\ss-id=\"(\\d+)\"").matcher(html);
        List<String> out = new ArrayList<>();
        while (m.find()) out.add(m.group(1));
        return out;
    }

    @Test
    void rendersAFragment() {
        try (SsrRenderer.Handle h = renderer.acquire()) {
            String out = h.renderFragment("<demo-counter start=\"5\" step=\"2\"></demo-counter>");
            assertThat(out).contains("<template shadowrootmode=\"open\">").doesNotContain("<html");
        }
    }

    /** Reset once per handle, before the first fragment: ids continue within a request, restart in the next. */
    @Test
    void fragmentIdsResetOncePerHandle() {
        String badge = "<demo-badge tone=\"success\">SSR</demo-badge>";
        for (int i = 0; i < 2; i++) {
            try (SsrRenderer.Handle h = renderer.acquire()) {
                assertThat(ids(h.renderFragment(badge))).containsExactly("1");
                assertThat(ids(h.renderFragment(badge))).containsExactly("2");
            }
        }
    }

    /** Full-document renders number their ids from 1 by themselves, without any reset. */
    @Test
    void documentIdsStartFromOne() {
        String page = "<html><body><demo-badge>a</demo-badge><demo-badge>b</demo-badge></body></html>";
        try (SsrRenderer.Handle h = renderer.acquire()) {
            for (int i = 0; i < 3; i++) {
                assertThat(ids(h.renderDocument(page))).containsExactly("1", "2");
            }
        }
    }

    /** Closing returns the instance to the pool, and closing twice is harmless. */
    @Test
    void closeReturnsTheInstance() {
        for (int i = 0; i < 5; i++) {
            SsrRenderer.Handle h = renderer.acquire();
            h.close();
            h.close();
        }
        try (SsrRenderer.Handle a = renderer.acquire(); SsrRenderer.Handle b = renderer.acquire()) {
            assertThat(ids(a.renderDocument("<demo-badge>a</demo-badge>"))).containsExactly("1");
            assertThat(ids(b.renderDocument("<demo-badge>b</demo-badge>"))).containsExactly("1");
        }
    }
}
