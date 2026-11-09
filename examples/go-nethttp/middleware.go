package main

import (
	"bytes"
	"mime"
	"net/http"
	"strconv"

	"ssr-wasm-examples/go-nethttp/internal/ssr"
)

// ssrMiddleware server-renders the Stencil components in text/html responses: it buffers the
// wrapped handler's response, passes the whole document through renderToString and writes the
// result. Other responses pass through untouched. main.go mounts it only on `/`, so helper pages
// aren't rendered twice.
func ssrMiddleware(renderer *ssr.Renderer, next http.Handler) http.Handler {
	return http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		bw := &bufferingWriter{ResponseWriter: w}
		next.ServeHTTP(bw, r)
		if !bw.buffering {
			return // passed through, or nothing written
		}

		h, err := renderer.Acquire(r.Context())
		if err != nil { // only when the request is cancelled while waiting for an instance
			http.Error(w, "service unavailable", http.StatusServiceUnavailable)
			return
		}
		html := h.Render(bw.buf.String(), nil)
		h.Release()

		w.Header().Set("Content-Length", strconv.Itoa(len(html)))
		w.WriteHeader(bw.status)
		w.Write([]byte(html))
	})
}

// bufferingWriter decides, when the status is written, whether to hold the body back for
// rendering (an uncompressed HTML response) or pass it straight through.
type bufferingWriter struct {
	http.ResponseWriter
	status    int
	buffering bool
	buf       bytes.Buffer
}

func (bw *bufferingWriter) WriteHeader(code int) {
	if bw.status != 0 {
		return
	}
	bw.status = code
	mt, _, _ := mime.ParseMediaType(bw.Header().Get("Content-Type"))
	bw.buffering = mt == "text/html" && bw.Header().Get("Content-Encoding") == ""
	if !bw.buffering {
		bw.ResponseWriter.WriteHeader(code)
	}
}

func (bw *bufferingWriter) Write(p []byte) (int, error) {
	if bw.status == 0 {
		bw.WriteHeader(http.StatusOK)
	}
	if bw.buffering {
		return bw.buf.Write(p)
	}
	return bw.ResponseWriter.Write(p)
}
