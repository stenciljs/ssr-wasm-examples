// Package ssr wraps the Stencil `ssr-wasm` Extism plugin for use from a Go HTTP server.
//
// The plugin is compiled once and instantiated poolSize times. An instance can't be used
// concurrently, so each request takes one out of the pool for its whole lifetime:
//
//	h, err := renderer.Acquire(r.Context()) // takes an instance
//	defer h.Release()                        // puts it back
//	html := h.Render(page, nil)              // or ssr.Fragment for {"fullDocument": false}
//
// The Handle calls resetSsrDocData once, before its first fragment, so a page's hydration ids
// start from 1 on every request.
package ssr

import (
	"context"
	"encoding/json"
	"fmt"
	"log/slog"
	"os"

	extism "github.com/extism/go-sdk"
	"github.com/tetratelabs/wazero"
)

// Options are Stencil's render options (see the `ssr` output target docs), sent as JSON.
type Options map[string]any

// Fragment renders only the components in the input, not a whole document.
var Fragment = Options{"fullDocument": false}

// Renderer owns the compiled plugin and the pool of instances.
type Renderer struct {
	compiled *extism.CompiledPlugin
	pool     chan *extism.Plugin
}

// New compiles the plugin once and fills the pool with poolSize instances.
func New(ctx context.Context, wasmPath string, poolSize int) (*Renderer, error) {
	manifest := extism.Manifest{
		Wasm:    []extism.Wasm{extism.WasmFile{Path: wasmPath}},
		Timeout: 30_000, // ms, per call
	}
	compiled, err := extism.NewCompiledPlugin(ctx, manifest, extism.PluginConfig{
		EnableWasi: true,
		// Abort a running call when the request's context is cancelled. wazero then closes
		// the instance for good, so Release replaces it.
		RuntimeConfig: wazero.NewRuntimeConfig().WithCloseOnContextDone(true),
	}, nil)
	if err != nil {
		return nil, fmt.Errorf("ssr: compile %s: %w", wasmPath, err)
	}

	r := &Renderer{compiled: compiled, pool: make(chan *extism.Plugin, poolSize)}
	for range poolSize {
		p, err := r.newInstance(ctx)
		if err != nil {
			return nil, err
		}
		r.pool <- p
	}
	return r, nil
}

func (r *Renderer) newInstance(ctx context.Context) (*extism.Plugin, error) {
	p, err := r.compiled.Instance(ctx, extism.PluginInstanceConfig{
		// Real clocks (so Date behaves as in a browser) and component console.* on stderr.
		ModuleConfig: wazero.NewModuleConfig().
			WithSysWalltime().
			WithSysNanotime().
			WithStdout(os.Stderr).
			WithStderr(os.Stderr),
	})
	if err != nil {
		return nil, fmt.Errorf("ssr: instantiate: %w", err)
	}
	// The first call initialises QuickJS and the components; pay that now, not on a request.
	if _, _, err := p.CallWithContext(ctx, "resetSsrDocData", nil); err != nil {
		p.Close(ctx)
		return nil, fmt.Errorf("ssr: warm up instance: %w", err)
	}
	return p, nil
}

// Acquire takes an instance from the pool, waiting until one is free or ctx is done.
// The returned Handle must be released.
func (r *Renderer) Acquire(ctx context.Context) (*Handle, error) {
	select {
	case p := <-r.pool:
		return &Handle{r: r, p: p, ctx: ctx}, nil
	case <-ctx.Done():
		return nil, ctx.Err()
	}
}

// Handle is one request's exclusive use of a plugin instance.
type Handle struct {
	r      *Renderer
	p      *extism.Plugin
	ctx    context.Context // the request's; cancels the plugin call if the client goes away
	reset  bool            // resetSsrDocData called for this request
	broken bool            // a call failed; the instance must not be reused
}

// Render passes html through renderToString. On a plugin `__error` or a failed call it logs
// and returns html unchanged, so the components still render on the client.
func (h *Handle) Render(html string, opts Options) string {
	if h.broken {
		return html
	}
	if opts == nil {
		opts = Options{}
	}
	if opts["fullDocument"] == false && !h.reset {
		if _, err := h.call("resetSsrDocData", nil); err != nil {
			slog.Error("ssr: resetSsrDocData failed", "err", err)
			return html
		}
		h.reset = true
	}
	input, _ := json.Marshal(map[string]any{"html": html, "options": opts})
	out, err := h.call("renderToString", input)
	if err == nil {
		html, err = parseResult(out, html)
	}
	if err != nil {
		slog.Error("ssr: renderToString failed, serving unrendered HTML", "err", err)
	}
	return html
}

func (h *Handle) call(fn string, input []byte) ([]byte, error) {
	_, out, err := h.p.CallWithContext(h.ctx, fn, input)
	if err != nil {
		h.broken = true // a trap or cancellation leaves the instance unusable
	}
	return out, err
}

// parseResult returns the html from renderToString's JSON result, or an error (and fallback)
// if the plugin reported `__error`.
func parseResult(out []byte, fallback string) (string, error) {
	var result struct {
		HTML  *string `json:"html"`
		Error any     `json:"__error"`
	}
	if err := json.Unmarshal(out, &result); err != nil {
		return fallback, fmt.Errorf("decoding result: %w", err)
	}
	if result.Error != nil {
		return fallback, fmt.Errorf("__error: %v", result.Error)
	}
	if result.HTML == nil {
		return fallback, fmt.Errorf("result has no html")
	}
	return *result.HTML, nil
}

// Release returns the instance to the pool, or replaces it if a call failed (otherwise the
// pool would shrink by one).
func (h *Handle) Release() {
	if !h.broken {
		h.r.pool <- h.p
		return
	}
	go func() {
		ctx := context.Background()
		h.p.Close(ctx)
		p, err := h.r.newInstance(ctx)
		if err != nil {
			slog.Error("ssr: replacing plugin instance failed", "err", err)
			return
		}
		h.r.pool <- p
	}()
}
