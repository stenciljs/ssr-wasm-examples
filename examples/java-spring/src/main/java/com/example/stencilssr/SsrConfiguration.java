package com.example.stencilssr;

import java.nio.file.Path;
import java.time.Duration;

import org.slf4j.Logger;
import org.slf4j.LoggerFactory;
import org.springframework.beans.factory.annotation.Value;
import org.springframework.context.annotation.Bean;
import org.springframework.context.annotation.Configuration;

import com.example.stencilssr.ssr.SsrRenderer;

/** The plugin pool, as a singleton bean. Spring calls {@code close()} on shutdown. */
@Configuration(proxyBeanMethods = false)
public class SsrConfiguration {

    private static final Logger log = LoggerFactory.getLogger(SsrConfiguration.class);

    @Bean
    SsrRenderer ssrRenderer(
            @Value("${ssr.wasm-path}") String wasmPath, // String: Spring's Path conversion treats it as a resource location
            @Value("${ssr.pool-size:0}") int poolSize,
            @Value("${ssr.acquire-timeout:30s}") Duration acquireTimeout,
            @Value("${ssr.call-timeout:30s}") Duration callTimeout) {
        // A render is pure CPU, so more instances than CPUs only queue on the same cores.
        // availableProcessors() follows the container's CPU limit.
        int size = poolSize > 0 ? poolSize : Runtime.getRuntime().availableProcessors();
        SsrRenderer r = new SsrRenderer(new SsrRenderer.Config(Path.of(wasmPath), size, acquireTimeout, callTimeout));
        log.info("ssr-wasm plugin ready: wasm={} pool={} instantiate={}",
                wasmPath, size, r.stats().instantiate().stream().map(d -> d.toMillis() + "ms").toList());
        return r;
    }
}
