// Package ssr wraps the Stencil `ssr-wasm` Extism plugin for use from a Go HTTP server.
//
// The plugin is compiled once (wazero compiles the ~6 MB module to native code ahead of time)
// and then instantiated a fixed number of times into a pool. An instance is not safe for
// concurrent use, so each request takes one out of the pool for its whole lifetime:
//
//	h, err := renderer.Acquire(r.Context()) // takes an instance
//	defer h.Release()                        // puts it back
//	html := h.Render(page, nil)              // or ssr.Fragment for {"fullDocument": false}
//
// Hydration ids: full-document renders always number them from 1. Fragments share one id
// sequence in the plugin, so the fragments of a page never collide; it keeps counting across
// requests unless reset. That's harmless, but the Handle resets it once, before its first
// fragment and never between fragments, so the same page renders the same way every time.
package ssr

import (
	"context"
	"encoding/json"
	"fmt"
	"io"
	"log/slog"
	"os"
	"sync"
	"time"

	extism "github.com/extism/go-sdk"
	"github.com/tetratelabs/wazero"
)

// Options are Stencil's render options (see the `ssr` output target docs), sent as JSON.
// Options that are functions, such as beforeSsr, can't be passed to the plugin.
type Options map[string]any

// Fragment renders only the components in the input, not a whole document.
var Fragment = Options{"fullDocument": false}

// Config configures a Renderer.
type Config struct {
	// WasmPath is the path to dist/ssr-wasm/index.wasm.
	WasmPath string
	// PoolSize is the number of plugin instances. Each holds its own QuickJS heap.
	PoolSize int
	// CacheDir, if set, enables wazero's on-disk compilation cache, so a restart can skip
	// compiling the module to native code.
	CacheDir string
	// CallTimeout bounds a single plugin call. Zero means 30s.
	CallTimeout time.Duration
	// PluginOutput receives the plugin's stdout/stderr (console.* from components).
	// Nil means os.Stderr.
	PluginOutput io.Writer
	Logger       *slog.Logger
}

// Stats are the startup timings, for logging.
type Stats struct {
	Compile     time.Duration
	Instantiate []time.Duration // per instance, including the plugin's first-call init
}

// Renderer owns the compiled plugin and the pool of instances.
type Renderer struct {
	compiled *extism.CompiledPlugin
	pool     chan *extism.Plugin
	cfg      Config
	log      *slog.Logger
	Stats    Stats
}

// New compiles the plugin and fills the pool. It blocks until every instance is ready, so
// the first requests don't pay for instantiation.
func New(ctx context.Context, cfg Config) (*Renderer, error) {
	if cfg.PoolSize < 1 {
		cfg.PoolSize = 1
	}
	if cfg.CallTimeout == 0 {
		cfg.CallTimeout = 30 * time.Second
	}
	if cfg.PluginOutput == nil {
		cfg.PluginOutput = os.Stderr
	}
	if cfg.Logger == nil {
		cfg.Logger = slog.Default()
	}

	// Close-on-context-done makes wazero abort a running call when its context is cancelled
	// (client went away, timeout). The price: the aborted instance is closed for good, so
	// Release replaces it instead of returning it to the pool.
	rc := wazero.NewRuntimeConfig().WithCloseOnContextDone(true)
	if cfg.CacheDir != "" {
		cache, err := wazero.NewCompilationCacheWithDir(cfg.CacheDir)
		if err != nil {
			return nil, fmt.Errorf("ssr: compilation cache %q: %w", cfg.CacheDir, err)
		}
		rc = rc.WithCompilationCache(cache)
	}

	manifest := extism.Manifest{
		Wasm:    []extism.Wasm{extism.WasmFile{Path: cfg.WasmPath}},
		Timeout: uint64(cfg.CallTimeout / time.Millisecond),
	}

	start := time.Now()
	compiled, err := extism.NewCompiledPlugin(ctx, manifest, extism.PluginConfig{
		EnableWasi:    true,
		RuntimeConfig: rc,
	}, nil)
	if err != nil {
		return nil, fmt.Errorf("ssr: compile %s: %w", cfg.WasmPath, err)
	}

	r := &Renderer{
		compiled: compiled,
		pool:     make(chan *extism.Plugin, cfg.PoolSize),
		cfg:      cfg,
		log:      cfg.Logger,
	}
	r.Stats.Compile = time.Since(start)

	for range cfg.PoolSize {
		start := time.Now()
		p, err := r.newInstance(ctx)
		if err != nil {
			r.Close(ctx)
			return nil, err
		}
		r.Stats.Instantiate = append(r.Stats.Instantiate, time.Since(start))
		r.pool <- p
	}
	return r, nil
}

func (r *Renderer) newInstance(ctx context.Context) (*extism.Plugin, error) {
	p, err := r.compiled.Instance(ctx, extism.PluginInstanceConfig{
		// Real clocks rather than wazero's deterministic fakes, so Date / performance.now()
		// in components behave as they would in a browser.
		ModuleConfig: wazero.NewModuleConfig().
			WithSysWalltime().
			WithSysNanotime().
			WithStdout(r.cfg.PluginOutput).
			WithStderr(r.cfg.PluginOutput),
	})
	if err != nil {
		return nil, fmt.Errorf("ssr: instantiate: %w", err)
	}
	// The first call into an extism-js plugin initialises QuickJS and evaluates the bundled
	// components. Pay that here rather than on someone's request.
	if _, _, err := p.CallWithContext(ctx, "resetSsrDocData", nil); err != nil {
		p.Close(ctx)
		return nil, fmt.Errorf("ssr: warm up instance: %w", err)
	}
	return p, nil
}

// Acquire takes an instance from the pool, waiting until one is free or ctx is done. The
// returned Handle must be released, and must not be used from more than one goroutine at a time.
func (r *Renderer) Acquire(ctx context.Context) (*Handle, error) {
	var p *extism.Plugin
	select {
	case p = <-r.pool:
	case <-ctx.Done():
		return nil, fmt.Errorf("ssr: waiting for a plugin instance: %w", context.Cause(ctx))
	}
	return &Handle{r: r, p: p, ctx: ctx}, nil
}

// Close closes the idle instances and the compiled module.
func (r *Renderer) Close(ctx context.Context) error {
	for {
		select {
		case p := <-r.pool:
			p.Close(ctx)
		default:
			return r.compiled.Close(ctx)
		}
	}
}

// replace swaps a broken instance for a fresh one, in the background so the request that
// broke it isn't held up.
func (r *Renderer) replace(old *extism.Plugin) {
	go func() {
		ctx := context.Background()
		old.Close(ctx)
		p, err := r.newInstance(ctx)
		if err != nil {
			// Leaves the pool one smaller; with no instances left, Acquire waits for its ctx.
			r.log.Error("ssr: replacing plugin instance failed", "err", err)
			return
		}
		r.pool <- p
	}()
}

// Handle is one request's exclusive use of a plugin instance.
type Handle struct {
	r       *Renderer
	p       *extism.Plugin
	ctx     context.Context // the request's; bounds every call made through the handle
	broken  bool
	reset   bool // fragment ids reset for this request
	once    sync.Once
	elapsed time.Duration
}

func (h *Handle) call(fn string, input []byte) ([]byte, error) {
	start := time.Now()
	_, out, err := h.p.CallWithContext(h.ctx, fn, input)
	h.elapsed += time.Since(start)
	if err != nil {
		// A trap or a cancelled context leaves the instance unusable (wazero closes it on
		// cancellation); don't hand it to another request.
		h.broken = true
	}
	return out, err
}

// Render passes html through renderToString and returns the rendered HTML. On any error -
// a plugin `__error` result or a failed call - it logs and returns html unchanged, so the
// components still render on the client.
func (h *Handle) Render(html string, opts Options) string {
	if h.broken || h.p == nil { // broken, or used after Release
		return html
	}
	if opts == nil {
		opts = Options{}
	}
	if full, ok := opts["fullDocument"].(bool); ok && !full && !h.reset {
		if _, err := h.call("resetSsrDocData", nil); err != nil {
			h.r.log.Error("ssr: resetSsrDocData failed", "err", err)
			return html
		}
		h.reset = true
	}
	input, err := json.Marshal(struct {
		HTML    string  `json:"html"`
		Options Options `json:"options"`
	}{html, opts})
	if err != nil {
		h.r.log.Error("ssr: encoding renderToString input", "err", err)
		return html
	}
	out, err := h.call("renderToString", input)
	if err != nil {
		if h.ctx.Err() != nil { // client went away or deadline passed: expected, not a plugin fault
			h.r.log.Warn("ssr: renderToString cancelled", "err", err)
		} else {
			h.r.log.Error("ssr: renderToString call failed", "err", err)
		}
		return html
	}
	rendered, err := parseResult(out)
	if err != nil {
		h.r.log.Error("ssr: renderToString failed, serving unrendered HTML", "err", err)
		return html
	}
	return rendered
}

// parseResult extracts the HTML from renderToString's JSON result, or the plugin's __error.
func parseResult(out []byte) (string, error) {
	var result struct {
		HTML  *string         `json:"html"`
		Error json.RawMessage `json:"__error"`
	}
	if err := json.Unmarshal(out, &result); err != nil {
		return "", fmt.Errorf("decoding result: %w", err)
	}
	if len(result.Error) > 0 && string(result.Error) != "null" {
		var msg string
		if json.Unmarshal(result.Error, &msg) != nil {
			msg = string(result.Error)
		}
		return "", fmt.Errorf("__error: %s", msg)
	}
	if result.HTML == nil {
		return "", fmt.Errorf("result has no html")
	}
	return *result.HTML, nil
}

// Elapsed is the time spent inside the plugin through this handle (reset + renders).
func (h *Handle) Elapsed() time.Duration { return h.elapsed }

// Release returns the instance to the pool (or replaces it if a call failed). It's safe to
// call more than once.
func (h *Handle) Release() {
	h.once.Do(func() {
		if h.broken {
			h.r.replace(h.p)
		} else {
			h.r.pool <- h.p
		}
		h.p = nil
	})
}
