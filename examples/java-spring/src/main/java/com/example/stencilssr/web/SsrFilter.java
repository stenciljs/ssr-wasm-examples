package com.example.stencilssr.web;

import java.io.IOException;
import java.nio.charset.Charset;

import org.springframework.stereotype.Component;
import org.springframework.web.filter.OncePerRequestFilter;
import org.springframework.web.util.ContentCachingResponseWrapper;

import com.example.stencilssr.ssr.SsrRenderer;

import jakarta.servlet.FilterChain;
import jakarta.servlet.ServletException;
import jakarta.servlet.http.HttpServletRequest;
import jakarta.servlet.http.HttpServletResponse;

/**
 * Middleware mode: buffers each response and passes {@code text/html} ones through
 * {@code renderToString} as a whole document. Spring Boot registers filter beans for every URL.
 *
 * <p>Pages that used the template helper ({@link StencilDialect}) are skipped, since their
 * components are already rendered; this filter closes the helper's handle when the request ends.
 */
@Component
public class SsrFilter extends OncePerRequestFilter {

    private final SsrRenderer renderer;

    SsrFilter(SsrRenderer renderer) {
        this.renderer = renderer;
    }

    @Override
    protected boolean shouldNotFilter(HttpServletRequest request) {
        String path = request.getRequestURI(); // never HTML, so don't buffer them
        return path.startsWith("/stencil/") || path.equals("/health");
    }

    @Override
    protected void doFilterInternal(HttpServletRequest request, HttpServletResponse response, FilterChain chain)
            throws ServletException, IOException {
        var buffered = new ContentCachingResponseWrapper(response);
        try {
            chain.doFilter(request, buffered);
            boolean usedHelper = request.getAttribute(StencilDialect.HANDLE_ATTRIBUTE) != null;
            String type = buffered.getContentType();
            if (!usedHelper && type != null && type.startsWith("text/html") && buffered.getContentSize() > 0) {
                Charset charset = Charset.forName(buffered.getCharacterEncoding());
                String html = new String(buffered.getContentAsByteArray(), charset);
                try (SsrRenderer.Handle h = renderer.acquire()) {
                    html = h.renderDocument(html);
                }
                buffered.resetBuffer();
                buffered.getOutputStream().write(html.getBytes(charset));
            }
        } finally {
            if (request.getAttribute(StencilDialect.HANDLE_ATTRIBUTE) instanceof SsrRenderer.Handle h) {
                h.close();
            }
            buffered.copyBodyToResponse(); // writes the (possibly replaced) body and Content-Length
        }
    }
}
