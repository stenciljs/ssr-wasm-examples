package com.example.stencilssr.web;

import java.nio.file.Path;

import org.springframework.beans.factory.annotation.Value;
import org.springframework.boot.web.servlet.FilterRegistrationBean;
import org.springframework.context.annotation.Bean;
import org.springframework.context.annotation.Configuration;
import org.springframework.core.Ordered;
import org.springframework.web.servlet.config.annotation.ResourceHandlerRegistry;
import org.springframework.web.servlet.config.annotation.WebMvcConfigurer;

import com.example.stencilssr.ssr.SsrRenderer;

@Configuration(proxyBeanMethods = false)
public class WebConfig implements WebMvcConfigurer {

    private final Path clientDir;

    WebConfig(@Value("${stencil.client-dir}") String clientDir) {
        this.clientDir = Path.of(clientDir).toAbsolutePath().normalize();
    }

    /** The client bundle under /stencil/, straight from STENCIL_CLIENT_DIR (read at startup). */
    @Override
    public void addResourceHandlers(ResourceHandlerRegistry registry) {
        registry.addResourceHandler("/stencil/**")
                .addResourceLocations("file:" + clientDir + "/"); // the trailing slash matters
    }

    /** Middleware mode, for every request (it only touches text/html responses). */
    @Bean
    FilterRegistrationBean<SsrFilter> ssrFilter(SsrRenderer renderer) {
        var reg = new FilterRegistrationBean<>(new SsrFilter(renderer));
        reg.addUrlPatterns("/*");
        // Inside Spring's own filters (character encoding, request context), so it sees the
        // response the way the servlet wrote it.
        reg.setOrder(Ordered.LOWEST_PRECEDENCE);
        return reg;
    }

    /** Helper mode. Spring Boot adds every IDialect bean to its Thymeleaf engine. */
    @Bean
    StencilDialect stencilDialect(SsrRequest ssrRequest) {
        return new StencilDialect(ssrRequest);
    }
}
