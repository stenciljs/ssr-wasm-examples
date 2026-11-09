// Command go-nethttp serves the ssr-wasm example pages with nothing but the Go standard library
// and the pure-Go Extism SDK (wazero, no cgo).
//
//	GET /         middleware mode: ssrMiddleware renders the whole HTML response
//	GET /helper   helper mode: the template renders each component through {{stencil ...}}
//	GET /health   "ok", without touching the plugin
//	GET /stencil/ the client bundle
package main

import (
	"context"
	"log/slog"
	"net/http"
	"os"
	"runtime"
	"strconv"

	"ssr-wasm-examples/go-nethttp/internal/ssr"
)

func main() {
	port := env("PORT", "8004")
	if len(os.Args) > 1 && os.Args[1] == "-healthcheck" {
		healthcheck(port) // the Docker HEALTHCHECK; the distroless image has no curl
	}

	wasmPath := env("SSR_WASM_PATH", "../../packages/components/dist/ssr-wasm/index.wasm")
	clientDir := env("STENCIL_CLIENT_DIR", "../../packages/components/dist/loader-bundle/demo-components")
	// A render is pure CPU work, so more instances than CPUs only queue.
	poolSize, err := strconv.Atoi(env("SSR_POOL_SIZE", strconv.Itoa(runtime.GOMAXPROCS(0))))
	if err != nil || poolSize < 1 {
		fatal("SSR_POOL_SIZE must be a positive integer")
	}

	renderer, err := ssr.New(context.Background(), wasmPath, poolSize)
	if err != nil {
		fatal(err.Error())
	}

	mux := http.NewServeMux()
	mux.Handle("GET /{$}", ssrMiddleware(renderer, http.HandlerFunc(indexHandler)))
	mux.Handle("GET /helper", helperHandler(renderer))
	mux.HandleFunc("GET /health", func(w http.ResponseWriter, r *http.Request) {
		w.Write([]byte("ok"))
	})
	mux.Handle("GET /stencil/", http.StripPrefix("/stencil/", http.FileServer(http.Dir(clientDir))))

	slog.Info("listening", "url", "http://localhost:"+port, "pool", poolSize)
	fatal(http.ListenAndServe(":"+port, mux).Error())
}

func healthcheck(port string) {
	res, err := http.Get("http://127.0.0.1:" + port + "/health")
	if err != nil || res.StatusCode != http.StatusOK {
		os.Exit(1)
	}
	os.Exit(0)
}

func env(key, def string) string {
	if v := os.Getenv(key); v != "" {
		return v
	}
	return def
}

func fatal(msg string) {
	slog.Error(msg)
	os.Exit(1)
}
