package com.example.stencilssr.web;

import java.nio.file.Path;

import org.springframework.beans.factory.annotation.Value;
import org.springframework.context.annotation.Bean;
import org.springframework.context.annotation.Configuration;
import org.springframework.web.servlet.config.annotation.ResourceHandlerRegistry;
import org.springframework.web.servlet.config.annotation.WebMvcConfigurer;

import com.example.stencilssr.ssr.SsrRenderer;

@Configuration(proxyBeanMethods = false)
public class WebConfig implements WebMvcConfigurer {

    private final Path clientDir;

    WebConfig(@Value("${stencil.client-dir}") String clientDir) {
        this.clientDir = Path.of(clientDir).toAbsolutePath().normalize();
    }

    /** The plugin pool, one instance per CPU by default: a render is pure CPU, so more only queue. */
    @Bean
    SsrRenderer ssrRenderer(@Value("${ssr.wasm-path}") String wasmPath, @Value("${ssr.pool-size:0}") int poolSize) {
        int size = poolSize > 0 ? poolSize : Runtime.getRuntime().availableProcessors();
        return new SsrRenderer(Path.of(wasmPath), size);
    }

    /** The client bundle under /stencil/, from STENCIL_CLIENT_DIR. */
    @Override
    public void addResourceHandlers(ResourceHandlerRegistry registry) {
        registry.addResourceHandler("/stencil/**")
                .addResourceLocations("file:" + clientDir + "/"); // the trailing slash matters
    }
}
