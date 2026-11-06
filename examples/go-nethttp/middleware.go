package main

import (
	"bytes"
	"fmt"
	"log/slog"
	"mime"
	"net/http"
	"strconv"
	"time"

	"ssr-wasm-examples/go-nethttp/internal/ssr"
)

// ssrMiddleware server-renders the Stencil components in text/html responses. It buffers the
// wrapped handler's response, passes the whole document through renderToString (default
// options, fullDocument: true) and writes the result. Anything else - other content types,
// already-encoded bodies - streams through untouched.
func ssrMiddleware(renderer *ssr.Renderer) func(http.Handler) http.Handler {
	return func(next http.Handler) http.Handler {
		return http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
			bw := &bufferingWriter{ResponseWriter: w}
			next.ServeHTTP(bw, r)
			if !bw.buffering {
				return // passed through, or nothing written
			}

			html := bw.buf.String()
			if h, err := renderer.Acquire(r.Context()); err != nil {
				slog.Error("ssr middleware: no plugin instance, serving unrendered HTML", "err", err)
			} else {
				html = h.Render(html, nil)
				w.Header().Set("Server-Timing", serverTiming(h.Elapsed()))
				h.Release()
			}
			w.Header().Set("Content-Length", strconv.Itoa(len(html)))
			w.WriteHeader(bw.status)
			w.Write([]byte(html))
		})
	}
}

// bufferingWriter decides, when the status is written, whether to hold the body back for
// rendering (an HTML response) or pass it straight through.
type bufferingWriter struct {
	http.ResponseWriter
	status      int
	wroteHeader bool
	buffering   bool
	buf         bytes.Buffer
}

func (bw *bufferingWriter) WriteHeader(code int) {
	if bw.wroteHeader {
		return
	}
	if code >= 100 && code < 200 { // informational: not the real response
		bw.ResponseWriter.WriteHeader(code)
		return
	}
	bw.wroteHeader = true
	bw.status = code
	h := bw.Header()
	mt, _, _ := mime.ParseMediaType(h.Get("Content-Type"))
	bw.buffering = mt == "text/html" && h.Get("Content-Encoding") == "" && code != http.StatusNoContent && code != http.StatusNotModified
	if bw.buffering {
		h.Del("Content-Length") // the rendered body has a different length
		return
	}
	bw.ResponseWriter.WriteHeader(code)
}

func (bw *bufferingWriter) Write(p []byte) (int, error) {
	if !bw.wroteHeader {
		if bw.Header().Get("Content-Type") == "" {
			// what net/http would do on the first Write
			bw.Header().Set("Content-Type", http.DetectContentType(p))
		}
		bw.WriteHeader(http.StatusOK)
	}
	if bw.buffering {
		return bw.buf.Write(p)
	}
	return bw.ResponseWriter.Write(p)
}

// Flush is a no-op while buffering; the whole document is needed before it can be rendered.
func (bw *bufferingWriter) Flush() {
	if bw.wroteHeader && !bw.buffering {
		if f, ok := bw.ResponseWriter.(http.Flusher); ok {
			f.Flush()
		}
	}
}

func serverTiming(d time.Duration) string {
	return fmt.Sprintf("ssr;desc=\"Stencil SSR WASM\";dur=%.2f", float64(d.Microseconds())/1000)
}
