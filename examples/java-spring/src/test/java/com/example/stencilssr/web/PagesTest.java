package com.example.stencilssr.web;

import static org.assertj.core.api.Assertions.assertThat;
import static org.junit.jupiter.api.Assumptions.assumeTrue;

import java.net.URI;
import java.net.http.HttpClient;
import java.net.http.HttpRequest;
import java.net.http.HttpResponse;
import java.nio.file.Files;

import org.junit.jupiter.api.BeforeAll;
import org.junit.jupiter.api.Test;
import org.springframework.boot.test.context.SpringBootTest;
import org.springframework.boot.test.web.server.LocalServerPort;

import com.example.stencilssr.ssr.SsrRendererTest;

/** The whole app on a random port, with the real plugin. */
@SpringBootTest(webEnvironment = SpringBootTest.WebEnvironment.RANDOM_PORT, properties = "ssr.pool-size=1")
class PagesTest {

    @LocalServerPort
    int port;

    final HttpClient http = HttpClient.newHttpClient();

    @BeforeAll
    static void needsPlugin() {
        assumeTrue(Files.isReadable(SsrRendererTest.wasmPath()), "plugin not found (set SSR_WASM_PATH)");
    }

    HttpResponse<String> get(String path) throws Exception {
        return http.send(HttpRequest.newBuilder(URI.create("http://localhost:" + port + path)).build(),
                HttpResponse.BodyHandlers.ofString());
    }

    @Test
    void middlewareRendersTheWholeDocument() throws Exception {
        var res = get("/");
        assertThat(res.statusCode()).isEqualTo(200);
        assertThat(res.body()).containsPattern("<html[^>]*data-stencil-build")
                .contains("Render with &lt;wasm&gt; &amp; \"friends\"")
                .contains("2 of 3 remaining");
    }

    /** The helper renders each component; the filter must then leave the page alone. */
    @Test
    void helperRendersFragmentsAndTheFilterSkipsThePage() throws Exception {
        var res = get("/helper");
        assertThat(res.statusCode()).isEqualTo(200);
        assertThat(res.body()).doesNotContainPattern("<html[^>]*data-stencil-build")
                .contains("<template shadowrootmode=\"open\">")
                .contains("Render with &lt;wasm&gt; &amp; \"friends\"");
        assertThat(SsrRendererTest.ids(res.body())).containsExactly("1", "2", "3", "4", "5", "6", "7", "8");
    }

    @Test
    void healthAndStaticFilesAreNotRendered() throws Exception {
        var health = get("/health");
        assertThat(health.body()).isEqualTo("ok");
        var js = get("/stencil/demo-components.esm.js");
        assertThat(js.statusCode()).isEqualTo(200);
        assertThat(js.headers().firstValue("content-type")).hasValueSatisfying(v -> assertThat(v).contains("javascript"));
    }
}
