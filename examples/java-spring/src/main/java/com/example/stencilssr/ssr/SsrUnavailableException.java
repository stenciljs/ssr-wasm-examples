package com.example.stencilssr.ssr;

/** No plugin instance became free in time. */
public class SsrUnavailableException extends RuntimeException {
    public SsrUnavailableException(String message) {
        super(message);
    }
}
