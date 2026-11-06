package com.example.stencilssr.web;

import java.io.IOException;
import java.nio.charset.Charset;
import java.nio.charset.StandardCharsets;

import org.slf4j.Logger;
import org.slf4j.LoggerFactory;
import org.springframework.http.HttpStatus;
import org.springframework.http.MediaType;
import org.springframework.web.filter.OncePerRequestFilter;
import org.springframework.web.util.ContentCachingResponseWrapper;

import com.example.stencilssr.ssr.SsrHandle;
import com.example.stencilssr.ssr.SsrOptions;
import com.example.stencilssr.ssr.SsrRenderer;
import com.example.stencilssr.ssr.SsrUnavailableException;

import jakarta.servlet.FilterChain;
import jakarta.servlet.ServletException;
import jakarta.servlet.http.HttpServletRequest;
import jakarta.servlet.http.HttpServletResponse;

/**
 * Middleware mode: server-renders the Stencil components in {@code text/html} responses. It
 * buffers the response, passes the whole document through {@code renderToString} (default
 * options, {@code fullDocument: true}) and writes the result. Anything else (other content
 * types, encoded bodies, error statuses without a body) is copied through untouched.
 *
 * <p>Pages that already rendered their components with the template helper are skipped: a
 * whole-document render would render those components a second time.
 */
public class SsrFilter extends OncePerRequestFilter {

    private static final Logger log = LoggerFactory.getLogger(SsrFilter.class);

    private final SsrRenderer renderer;

    public SsrFilter(SsrRenderer renderer) {
        this.renderer = renderer;
    }

    @Override
    protected boolean shouldNotFilter(HttpServletRequest request) {
        // The client bundle and health check are never HTML; don't buffer them.
        String path = request.getRequestURI().substring(request.getContextPath().length());
        return path.startsWith("/stencil/") || path.equals("/health");
    }

    @Override
    protected void doFilterInternal(HttpServletRequest request, HttpServletResponse response, FilterChain chain)
            throws ServletException, IOException {
        var wrapper = new ContentCachingResponseWrapper(response);
        try {
            chain.doFilter(request, wrapper);
            if (shouldRender(request, wrapper)) {
                render(wrapper);
            }
        } finally {
            wrapper.copyBodyToResponse(); // writes the (possibly replaced) body and Content-Length
        }
    }

    private boolean shouldRender(HttpServletRequest request, ContentCachingResponseWrapper response) {
        if (request.getAttribute(SsrRequest.USED_ATTRIBUTE) != null) {
            return false; // the template helper already rendered this page's components
        }
        if (response.getContentType() == null || response.getContentSize() == 0) {
            return false;
        }
        MediaType type;
        try {
            type = MediaType.parseMediaType(response.getContentType());
        } catch (IllegalArgumentException e) {
            return false;
        }
        return MediaType.TEXT_HTML.isCompatibleWith(type)
                && response.getHeader("Content-Encoding") == null
                && response.getStatus() != HttpStatus.NO_CONTENT.value()
                && response.getStatus() != HttpStatus.NOT_MODIFIED.value();
    }

    private void render(ContentCachingResponseWrapper response) throws IOException {
        Charset charset = response.getCharacterEncoding() != null
                ? Charset.forName(response.getCharacterEncoding())
                : StandardCharsets.UTF_8;
        String html = new String(response.getContentAsByteArray(), charset);
        String rendered;
        try (SsrHandle h = renderer.acquire()) {
            rendered = h.render(html, SsrOptions.DOCUMENT);
            response.setHeader("Server-Timing", ServerTiming.ssr(h.elapsed()));
        } catch (SsrUnavailableException e) {
            log.error("ssr filter: {}, serving unrendered HTML", e.getMessage());
            return;
        }
        response.resetBuffer();
        response.getOutputStream().write(rendered.getBytes(charset));
    }
}
