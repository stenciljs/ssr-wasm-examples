package com.example.stencilssr;

import org.springframework.boot.SpringApplication;
import org.springframework.boot.autoconfigure.SpringBootApplication;

/**
 * Spring Boot + Thymeleaf serving the ssr-wasm example pages.
 *
 * <ul>
 *   <li>{@code GET /}: middleware mode; {@code SsrFilter} renders the whole HTML response</li>
 *   <li>{@code GET /helper}: helper mode; the template renders each component through {@code ssr:render}</li>
 *   <li>{@code GET /health}: {@code ok}, without touching the plugin</li>
 *   <li>{@code GET /stencil/**}: the client bundle, from {@code STENCIL_CLIENT_DIR}</li>
 * </ul>
 */
@SpringBootApplication
public class StencilSsrApplication {

    public static void main(String[] args) {
        SpringApplication.run(StencilSsrApplication.class, args);
    }
}
