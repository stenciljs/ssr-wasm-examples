package com.example.stencilssr.ssr;

import static org.assertj.core.api.Assertions.assertThat;
import static org.assertj.core.api.Assertions.assertThatThrownBy;
import static org.junit.jupiter.api.Assumptions.assumeTrue;

import java.nio.charset.StandardCharsets;
import java.nio.file.Files;
import java.nio.file.Path;
import java.time.Duration;
import java.util.ArrayList;
import java.util.List;
import java.util.regex.Pattern;

import org.junit.jupiter.api.AfterAll;
import org.junit.jupiter.api.BeforeAll;
import org.junit.jupiter.api.Test;

import tools.jackson.databind.json.JsonMapper;

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
        renderer = new SsrRenderer(new SsrRenderer.Config(wasmPath(), 2, Duration.ofMillis(100), Duration.ofSeconds(30)));
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
        try (SsrHandle h = renderer.acquire()) {
            String out = h.render("<demo-counter start=\"5\" step=\"2\"></demo-counter>", SsrOptions.FRAGMENT);
            assertThat(out).contains("<template shadowrootmode=\"open\">").doesNotContain("<html");
            assertThat(h.elapsed()).isPositive();
        }
    }

    /** Reset once per handle, before the first fragment: ids continue within a request, restart in the next. */
    @Test
    void fragmentIdsResetOncePerHandle() {
        String badge = "<demo-badge tone=\"success\">SSR</demo-badge>";
        for (int i = 0; i < 2; i++) {
            try (SsrHandle h = renderer.acquire()) {
                assertThat(ids(h.render(badge, SsrOptions.FRAGMENT))).containsExactly("1");
                assertThat(ids(h.render(badge, SsrOptions.FRAGMENT))).containsExactly("2");
            }
        }
    }

    /** Full-document renders number their ids from 1 by themselves, without any reset. */
    @Test
    void documentIdsStartFromOne() {
        String page = "<html><body><demo-badge>a</demo-badge><demo-badge>b</demo-badge></body></html>";
        try (SsrHandle h = renderer.acquire()) {
            for (int i = 0; i < 3; i++) {
                assertThat(ids(h.render(page, SsrOptions.DOCUMENT))).containsExactly("1", "2");
            }
        }
    }

    @Test
    void acquireTimesOutWhenThePoolIsEmpty() {
        List<SsrHandle> held = new ArrayList<>();
        for (int i = 0; i < renderer.config().poolSize(); i++) held.add(renderer.acquire());
        assertThatThrownBy(renderer::acquire).isInstanceOf(SsrUnavailableException.class);
        for (SsrHandle h : held) {
            h.close();
            h.close(); // idempotent
        }
        assertThat(renderer.available()).isEqualTo(renderer.config().poolSize());
    }

    @Test
    void parseResult() {
        JsonMapper json = JsonMapper.builder().build();
        assertThat(SsrHandle.parseResult(json, bytes("{\"html\":\"<p>ok</p>\",\"hydratedCount\":0}"))).isEqualTo("<p>ok</p>");
        for (String bad : List.of("{\"__error\":\"ReferenceError: x\"}", "{\"__error\":{\"message\":\"x\"}}", "{}", "not json")) {
            assertThatThrownBy(() -> SsrHandle.parseResult(json, bytes(bad))).as(bad).isInstanceOf(SsrHandle.RenderException.class);
        }
    }

    private static byte[] bytes(String s) {
        return s.getBytes(StandardCharsets.UTF_8);
    }
}
