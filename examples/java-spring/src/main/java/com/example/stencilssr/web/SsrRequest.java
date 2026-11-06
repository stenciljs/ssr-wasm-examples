package com.example.stencilssr.web;

import org.slf4j.Logger;
import org.slf4j.LoggerFactory;
import org.springframework.stereotype.Component;
import org.springframework.web.context.annotation.RequestScope;

import com.example.stencilssr.ssr.SsrHandle;
import com.example.stencilssr.ssr.SsrOptions;
import com.example.stencilssr.ssr.SsrRenderer;
import com.example.stencilssr.ssr.SsrUnavailableException;

import jakarta.annotation.PreDestroy;
import jakarta.servlet.http.HttpServletRequest;
import jakarta.servlet.http.HttpServletResponse;

/**
 * The current request's plugin handle, for the template helper. Request-scoped: the first
 * {@code ssr:render} in a page acquires an instance, every later one in the same page reuses
 * it (and its hydration-id sequence, reset once before the first fragment), and Spring releases
 * it when the request completes.
 */
@Component
@RequestScope
public class SsrRequest {

    /** Request attribute set once the helper has rendered something; {@link SsrFilter} then leaves the page alone. */
    static final String USED_ATTRIBUTE = SsrRequest.class.getName() + ".used";

    private static final Logger log = LoggerFactory.getLogger(SsrRequest.class);

    private final SsrRenderer renderer;
    private final HttpServletRequest request;
    private final HttpServletResponse response;
    private SsrHandle handle;
    private boolean unavailable;

    SsrRequest(SsrRenderer renderer, HttpServletRequest request, HttpServletResponse response) {
        this.renderer = renderer;
        this.request = request;
        this.response = response;
    }

    /** Renders one component's markup as a fragment ({@code fullDocument: false}). */
    public String renderFragment(String markup) {
        request.setAttribute(USED_ATTRIBUTE, Boolean.TRUE);
        if (handle == null && !unavailable) {
            try {
                handle = renderer.acquire();
            } catch (SsrUnavailableException e) {
                log.error("ssr helper: {}, serving unrendered markup", e.getMessage());
                unavailable = true; // don't wait again for every component on the page
            }
        }
        if (handle == null) {
            return markup;
        }
        String html = handle.render(markup, SsrOptions.FRAGMENT);
        // Thymeleaf buffers the whole page (produce-partial-output-while-processing=false), so
        // the response isn't committed yet and the header can be updated after each fragment.
        if (!response.isCommitted()) {
            response.setHeader("Server-Timing", ServerTiming.ssr(handle.elapsed()));
        }
        return html;
    }

    @PreDestroy
    void release() {
        if (handle != null) {
            handle.close();
        }
    }
}
