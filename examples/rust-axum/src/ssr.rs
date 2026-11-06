//! A wrapper around the Stencil `ssr-wasm` Extism plugin for use from an async Rust server.
//!
//! The plugin is compiled once (`extism::CompiledPlugin`: wasmtime compiles the ~6 MB module to
//! native code) and instantiated a fixed number of times into a pool. A `Plugin` runs one call at
//! a time, so each request takes one out of the pool for its whole lifetime:
//!
//! ```ignore
//! let html = renderer
//!     .run(move |mut h| h.render(&page, &Options::new())) // or ssr::fragment()
//!     .await?;
//! ```
//!
//! Plugin calls are blocking CPU work (the plugin is QuickJS interpreting the components), so
//! [`Renderer::run`] waits for a free instance asynchronously and then runs the closure on
//! tokio's blocking thread pool, never on an async worker thread. The [`Handle`] is moved into the
//! closure and gives its instance back to the pool when it's dropped there.
//!
//! Hydration ids: full-document renders always number them from 1. Fragments share one id
//! sequence in the plugin, so the fragments of a page never collide; it keeps counting across
//! requests unless reset. That's harmless, but the `Handle` resets it once, before its first
//! fragment and never between fragments, so the same page renders the same way every time.

use std::{
    fmt,
    path::PathBuf,
    sync::{Arc, Mutex},
    time::{Duration, Instant},
};

use anyhow::Context as _;
use extism::{CompiledPlugin, Manifest, Plugin, PluginBuilder, Wasm};
use serde::Deserialize;
use tokio::sync::{OwnedSemaphorePermit, Semaphore};

/// Stencil's render options (see the `ssr` output target docs), sent as JSON. Options that are
/// functions, such as `beforeSsr`, can't be passed to the plugin.
pub type Options = serde_json::Map<String, serde_json::Value>;

/// `{"fullDocument": false}`: render only the components in the input, not a whole document.
pub fn fragment() -> Options {
    let mut o = Options::new();
    o.insert("fullDocument".into(), false.into());
    o
}

/// Configures a [`Renderer`].
#[derive(Debug, Clone)]
pub struct Config {
    /// Path to `dist/ssr-wasm/index.wasm`.
    pub wasm_path: PathBuf,
    /// Number of plugin instances. Each holds its own QuickJS heap.
    pub pool_size: usize,
    /// A wasmtime cache config file (TOML). `None` disables the compilation cache.
    pub cache_config: Option<PathBuf>,
    /// Bounds a single plugin call (the manifest's `timeout_ms`).
    pub call_timeout: Duration,
    /// How long a request waits for a free instance before giving up.
    pub acquire_timeout: Duration,
}

impl Config {
    pub fn new(wasm_path: impl Into<PathBuf>) -> Self {
        Self {
            wasm_path: wasm_path.into(),
            pool_size: std::thread::available_parallelism().map_or(1, usize::from),
            cache_config: None,
            call_timeout: Duration::from_secs(30),
            acquire_timeout: Duration::from_secs(30),
        }
    }
}

/// Startup timings, for logging.
#[derive(Debug, Clone, Default)]
pub struct Stats {
    pub compile: Duration,
    /// Per instance, including the plugin's first call (QuickJS start-up).
    pub instantiate: Vec<Duration>,
}

#[derive(Debug)]
pub enum Error {
    /// No instance became free within `acquire_timeout`.
    Busy,
    /// Every instance was lost (replacements failed); the pool is closed.
    Closed,
    /// The closure passed to [`Renderer::run`] panicked.
    Panicked,
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Error::Busy => "ssr: timed out waiting for a plugin instance",
            Error::Closed => "ssr: no plugin instances left",
            Error::Panicked => "ssr: render panicked",
        })
    }
}

impl std::error::Error for Error {}

/// Owns the compiled plugin and the pool of instances. Cheap to clone.
#[derive(Clone)]
pub struct Renderer {
    inner: Arc<Inner>,
    acquire_timeout: Duration,
    pub stats: Stats,
}

struct Inner {
    compiled: CompiledPlugin,
    /// Idle instances. Invariant: there are at least as many as `permits` has available, so
    /// whoever holds a permit can always pop one.
    idle: Mutex<Vec<Plugin>>,
    /// One permit per instance. Waiting is async and FIFO, and drops out cleanly when the
    /// request's future is dropped (client went away).
    permits: Arc<Semaphore>,
}

impl Renderer {
    /// Compiles the plugin and fills the pool. Blocks (seconds, without a warm cache) until
    /// every instance is ready, so the first requests don't pay for instantiation. Call it before
    /// starting to serve, or from `spawn_blocking`.
    pub fn new(cfg: &Config) -> anyhow::Result<Self> {
        let pool_size = cfg.pool_size.max(1);
        let manifest =
            Manifest::new([Wasm::file(&cfg.wasm_path)]).with_timeout(cfg.call_timeout);
        let mut builder = PluginBuilder::new(manifest).with_wasi(true);
        // Without either call, Extism enables wasmtime's cache anyway, in ~/.cache/wasmtime
        // (or wherever $EXTISM_CACHE_CONFIG says), and fails to compile if it can't create it.
        builder = match &cfg.cache_config {
            Some(path) => builder.with_cache_config(path),
            None => builder.with_cache_disabled(),
        };

        let start = Instant::now();
        let compiled = CompiledPlugin::new(builder)
            .with_context(|| format!("ssr: compile {}", cfg.wasm_path.display()))?;
        let mut stats = Stats { compile: start.elapsed(), ..Stats::default() };

        let inner = Arc::new(Inner {
            compiled,
            idle: Mutex::new(Vec::with_capacity(pool_size)),
            permits: Arc::new(Semaphore::new(0)),
        });
        for _ in 0..pool_size {
            let start = Instant::now();
            let p = inner.new_instance()?;
            stats.instantiate.push(start.elapsed());
            inner.idle().push(p);
            inner.permits.add_permits(1);
        }
        Ok(Self { inner, acquire_timeout: cfg.acquire_timeout, stats })
    }

    /// Takes an instance from the pool (waiting asynchronously for one to be free), then runs `f`
    /// with it on the blocking thread pool. The instance goes back to the pool when the
    /// [`Handle`] is dropped, normally at the end of `f`.
    ///
    /// If the calling future is dropped while `f` is running (the client went away), `f` still
    /// runs to completion: wasm can't be stopped at an arbitrary point without losing the instance.
    pub async fn run<R, F>(&self, f: F) -> Result<R, Error>
    where
        F: FnOnce(Handle) -> R + Send + 'static,
        R: Send + 'static,
    {
        let permit = tokio::time::timeout(
            self.acquire_timeout,
            self.inner.permits.clone().acquire_owned(),
        )
        .await
        .map_err(|_| Error::Busy)?
        .map_err(|_| Error::Closed)?;
        let plugin = self.inner.idle().pop().expect("a permit guarantees an idle instance");
        let handle = Handle {
            inner: self.inner.clone(),
            plugin: Some(plugin),
            permit: Some(permit),
            reset: false,
            broken: false,
            elapsed: Duration::ZERO,
        };
        tokio::task::spawn_blocking(move || f(handle))
            .await
            .map_err(|_| Error::Panicked)
    }

    #[cfg_attr(not(test), allow(dead_code))]
    /// A clone that gives up waiting for an instance after `d` instead.
    pub fn with_acquire_timeout(mut self, d: Duration) -> Self {
        self.acquire_timeout = d;
        self
    }

    #[cfg_attr(not(test), allow(dead_code))]
    /// Number of idle instances.
    pub fn idle(&self) -> usize {
        self.inner.permits.available_permits()
    }
}

impl Inner {
    fn idle(&self) -> std::sync::MutexGuard<'_, Vec<Plugin>> {
        // Nothing panics while holding the lock, but don't take the server down if it did.
        self.idle.lock().unwrap_or_else(|e| e.into_inner())
    }

    fn new_instance(&self) -> anyhow::Result<Plugin> {
        let mut p = Plugin::new_from_compiled(&self.compiled).context("ssr: instantiate")?;
        // Extism instantiates lazily, and the first call into an extism-js plugin starts QuickJS
        // and evaluates the bundled components. Pay that here rather than on someone's request.
        p.call::<&str, &[u8]>("resetSsrDocData", "").context("ssr: warm up instance")?;
        Ok(p)
    }

    /// Swaps a broken instance for a fresh one, on its own thread so the request that broke it
    /// isn't held up. The permit is held until the replacement is in the pool.
    fn replace(self: Arc<Self>, old: Plugin, permit: OwnedSemaphorePermit) {
        std::thread::spawn(move || {
            drop(old);
            match self.new_instance() {
                Ok(p) => {
                    self.idle().push(p);
                    drop(permit);
                }
                Err(e) => {
                    // The pool stays one smaller. With none left, `run` times out with `Busy`.
                    tracing::error!("ssr: replacing plugin instance failed: {e:#}");
                    permit.forget();
                }
            }
        });
    }
}

/// One request's exclusive use of a plugin instance. Returns it to the pool when dropped.
pub struct Handle {
    inner: Arc<Inner>,
    plugin: Option<Plugin>,
    permit: Option<OwnedSemaphorePermit>,
    /// Fragment ids have been reset for this request.
    reset: bool,
    /// A call failed (trap, timeout): don't trust the instance again.
    broken: bool,
    elapsed: Duration,
}

impl fmt::Debug for Handle {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Handle")
            .field("reset", &self.reset)
            .field("broken", &self.broken)
            .field("elapsed", &self.elapsed)
            .finish_non_exhaustive()
    }
}

impl Handle {
    fn call(&mut self, function: &str, input: &str) -> anyhow::Result<String> {
        let plugin = self.plugin.as_mut().expect("plugin is only taken on drop");
        let start = Instant::now();
        let out = plugin.call::<&str, &str>(function, input).map(str::to_owned);
        self.elapsed += start.elapsed();
        if out.is_err() {
            // A trap or a timeout can leave QuickJS mid-way through something; don't hand the
            // instance to another request.
            self.broken = true;
        }
        out
    }

    /// Passes `html` through `renderToString` and returns the rendered HTML. On any error - a
    /// plugin `__error` result or a failed call - it logs and returns `html` unchanged, so the
    /// components still render on the client.
    pub fn render(&mut self, html: &str, options: &Options) -> String {
        if self.broken {
            return html.to_owned();
        }
        let is_fragment = options.get("fullDocument") == Some(&serde_json::Value::Bool(false));
        if is_fragment && !self.reset {
            if let Err(e) = self.call("resetSsrDocData", "") {
                tracing::error!("ssr: resetSsrDocData failed: {e:#}");
                return html.to_owned();
            }
            self.reset = true;
        }
        let input = serde_json::json!({ "html": html, "options": options }).to_string();
        let out = match self.call("renderToString", &input) {
            Ok(out) => out,
            Err(e) => {
                tracing::error!("ssr: renderToString call failed, serving unrendered HTML: {e:#}");
                return html.to_owned();
            }
        };
        match parse_result(&out) {
            Ok(rendered) => rendered,
            Err(e) => {
                tracing::error!("ssr: renderToString failed, serving unrendered HTML: {e:#}");
                html.to_owned()
            }
        }
    }

    /// Time spent inside the plugin through this handle (reset + renders).
    pub fn elapsed(&self) -> Duration {
        self.elapsed
    }
}

impl Drop for Handle {
    fn drop(&mut self) {
        let (Some(plugin), Some(permit)) = (self.plugin.take(), self.permit.take()) else {
            return;
        };
        if self.broken || std::thread::panicking() {
            self.inner.clone().replace(plugin, permit);
        } else {
            self.inner.idle().push(plugin);
            drop(permit); // after the push, keeping the pool invariant
        }
    }
}

/// Extracts the HTML from `renderToString`'s JSON result, or the plugin's `__error`.
pub fn parse_result(out: &str) -> anyhow::Result<String> {
    #[derive(Deserialize)]
    struct RenderResult {
        html: Option<String>,
        #[serde(rename = "__error")]
        error: Option<serde_json::Value>,
    }
    let result: RenderResult = serde_json::from_str(out).context("decoding result")?;
    if let Some(err) = result.error {
        let msg = match err {
            serde_json::Value::String(s) => s,
            other => other.to_string(),
        };
        anyhow::bail!("__error: {msg}");
    }
    result.html.context("result has no html")
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::OnceLock;

    /// Shared by the tests: compiling the module takes seconds.
    fn renderer() -> Option<Renderer> {
        static SHARED: OnceLock<Option<Renderer>> = OnceLock::new();
        SHARED
            .get_or_init(|| {
                let path = std::env::var("SSR_WASM_PATH").unwrap_or_else(|_| {
                    "../../packages/components/dist/ssr-wasm/index.wasm".into()
                });
                if !std::path::Path::new(&path).exists() {
                    eprintln!("plugin not found at {path} (set SSR_WASM_PATH); skipping");
                    return None;
                }
                let mut cfg = Config::new(path);
                cfg.pool_size = 2;
                cfg.cache_config = std::env::var_os("SSR_CACHE_CONFIG").map(Into::into);
                Some(Renderer::new(&cfg).expect("renderer"))
            })
            .clone()
    }

    fn ids(html: &str) -> Vec<u32> {
        html.split(" s-id=\"")
            .skip(1)
            .map(|s| s[..s.find('"').unwrap()].parse().unwrap())
            .collect()
    }

    #[tokio::test]
    async fn fragment_render() {
        let Some(r) = renderer() else { return };
        let (out, elapsed) = r
            .run(|mut h| {
                let out = h.render(r#"<demo-counter start="5" step="2"></demo-counter>"#, &fragment());
                (out, h.elapsed())
            })
            .await
            .unwrap();
        assert!(out.contains(r#"<template shadowrootmode="open">"#), "{out}");
        assert!(!out.contains("<html"), "a fragment render shouldn't produce a document: {out}");
        assert!(elapsed > Duration::ZERO);
    }

    /// Fragment ids reset once per handle, before the first fragment: fragments in one request
    /// continue the sequence, and the next request starts again from 1.
    #[tokio::test]
    async fn fragment_ids_reset_once_per_handle() {
        let Some(r) = renderer() else { return };
        let frag = r#"<demo-badge tone="success">SSR</demo-badge>"#;
        for _ in 0..3 {
            let (a, b) = r
                .run(move |mut h| (ids(&h.render(frag, &fragment())), ids(&h.render(frag, &fragment()))))
                .await
                .unwrap();
            assert_eq!((a, b), (vec![1], vec![2]));
        }
    }

    /// Full-document renders number their ids from 1 by themselves, without any reset.
    #[tokio::test]
    async fn document_ids_start_from_one() {
        let Some(r) = renderer() else { return };
        let page = "<html><body><demo-badge>a</demo-badge><demo-badge>b</demo-badge></body></html>";
        let all = r
            .run(move |mut h| (0..3).map(|_| ids(&h.render(page, &Options::new()))).collect::<Vec<_>>())
            .await
            .unwrap();
        assert!(all.iter().all(|ids| ids == &[1, 2]), "{all:?}");
    }

    #[tokio::test]
    async fn waits_for_a_free_instance_then_times_out() {
        let Some(r) = renderer() else { return };
        let r = r.with_acquire_timeout(Duration::from_millis(200));
        // Hold every instance from blocking threads, then ask for one more.
        let (tx, rx) = std::sync::mpsc::channel::<()>();
        let rx = Arc::new(Mutex::new(rx));
        let mut held = Vec::new();
        for _ in 0..2 {
            let rx = rx.clone();
            let r = r.clone();
            held.push(tokio::spawn(async move {
                r.run(move |_h| { let _ = rx.lock().unwrap().recv(); }).await.unwrap()
            }));
        }
        while r.idle() > 0 {
            tokio::time::sleep(Duration::from_millis(5)).await;
        }
        assert!(matches!(r.run(|_| ()).await, Err(Error::Busy)));
        drop(tx);
        for h in held {
            h.await.unwrap();
        }
        assert_eq!(r.idle(), 2);
    }

    #[test]
    fn parse_result_cases() {
        assert_eq!(parse_result(r#"{"html":"<p>ok</p>","hydratedCount":0}"#).unwrap(), "<p>ok</p>");
        assert_eq!(parse_result(r#"{"html":"<p>ok</p>","__error":null}"#).unwrap(), "<p>ok</p>");
        for bad in [r#"{"__error":"ReferenceError: x"}"#, r#"{"__error":{"message":"x"}}"#, "{}", "not json"] {
            assert!(parse_result(bad).is_err(), "{bad}");
        }
    }
}
