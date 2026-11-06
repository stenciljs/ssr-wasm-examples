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
	"errors"
	"flag"
	"fmt"
	"log/slog"
	"net/http"
	"os"
	"os/signal"
	"runtime"
	"strconv"
	"strings"
	"syscall"
	"time"

	"ssr-wasm-examples/go-nethttp/internal/ssr"
)

func main() {
	warmCache := flag.Bool("warm-cache", false, "compile the plugin into SSR_CACHE_DIR and exit (used by the Dockerfile)")
	healthcheck := flag.Bool("healthcheck", false, "GET /health on the local server and exit non-zero on failure")
	flag.Parse()

	port := env("PORT", "8004")
	if *healthcheck {
		os.Exit(checkHealth(port))
	}

	cfg := ssr.Config{
		WasmPath: env("SSR_WASM_PATH", "../../packages/components/dist/ssr-wasm/index.wasm"),
		PoolSize: envInt("SSR_POOL_SIZE", runtime.GOMAXPROCS(0)),
		CacheDir: os.Getenv("SSR_CACHE_DIR"),
	}
	clientDir := env("STENCIL_CLIENT_DIR", "../../packages/components/dist/loader-bundle/demo-components")

	ctx, stop := signal.NotifyContext(context.Background(), os.Interrupt, syscall.SIGTERM)
	defer stop()

	if *warmCache {
		if cfg.CacheDir == "" {
			fatal("-warm-cache needs SSR_CACHE_DIR")
		}
		cfg.PoolSize = 1
	}

	renderer, err := ssr.New(ctx, cfg)
	if err != nil {
		fatal(err.Error())
	}
	slog.Info("ssr-wasm plugin ready",
		"wasm", cfg.WasmPath,
		"compile", renderer.Stats.Compile.Round(time.Millisecond),
		"instantiate", roundAll(renderer.Stats.Instantiate),
		"pool", cfg.PoolSize,
		"cache", cfg.CacheDir)
	if *warmCache {
		renderer.Close(ctx)
		return
	}

	mux := http.NewServeMux()
	mux.Handle("GET /{$}", ssrMiddleware(renderer)(http.HandlerFunc(indexHandler)))
	mux.Handle("GET /helper", helperHandler(renderer))
	mux.HandleFunc("GET /health", func(w http.ResponseWriter, r *http.Request) {
		w.Header().Set("Content-Type", "text/plain; charset=utf-8")
		w.Write([]byte("ok"))
	})
	mux.Handle("GET /stencil/", http.StripPrefix("/stencil/", noDirListing(http.FileServer(http.Dir(clientDir)))))

	srv := &http.Server{
		Addr:              ":" + port,
		Handler:           mux,
		ReadHeaderTimeout: 10 * time.Second,
	}
	go func() {
		<-ctx.Done()
		shutdownCtx, cancel := context.WithTimeout(context.Background(), 10*time.Second)
		defer cancel()
		srv.Shutdown(shutdownCtx)
	}()

	slog.Info("listening", "url", "http://localhost:"+port)
	if err := srv.ListenAndServe(); !errors.Is(err, http.ErrServerClosed) {
		fatal(err.Error())
	}
	renderer.Close(context.Background())
}

func noDirListing(next http.Handler) http.Handler {
	return http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		if r.URL.Path == "" || strings.HasSuffix(r.URL.Path, "/") {
			http.NotFound(w, r)
			return
		}
		next.ServeHTTP(w, r)
	})
}

// checkHealth is the container HEALTHCHECK; the runtime image has no curl or shell.
func checkHealth(port string) int {
	c := http.Client{Timeout: 3 * time.Second}
	res, err := c.Get("http://127.0.0.1:" + port + "/health")
	if err != nil {
		fmt.Fprintln(os.Stderr, err)
		return 1
	}
	res.Body.Close()
	if res.StatusCode != http.StatusOK {
		return 1
	}
	return 0
}

func env(key, def string) string {
	if v := os.Getenv(key); v != "" {
		return v
	}
	return def
}

func envInt(key string, def int) int {
	if v := os.Getenv(key); v != "" {
		n, err := strconv.Atoi(v)
		if err != nil || n < 1 {
			fatal(key + " must be a positive integer")
		}
		return n
	}
	return def
}

func roundAll(ds []time.Duration) []string {
	out := make([]string, len(ds))
	for i, d := range ds {
		out[i] = d.Round(time.Millisecond).String()
	}
	return out
}

func fatal(msg string) {
	slog.Error(msg)
	os.Exit(1)
}
