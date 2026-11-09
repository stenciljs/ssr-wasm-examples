//! The Stencil `ssr-wasm` Extism plugin, shared by an async server.
//!
//! The plugin is compiled once (`CompiledPlugin`) and instantiated `pool_size` times. A `Plugin`
//! runs one call at a time, so the instances wait in a channel and each request takes one for its
//! whole lifetime:
//!
//! ```ignore
//! let html = renderer.run(move |mut s| s.render_document(&page)).await?;
//! ```
//!
//! Plugin calls are blocking CPU work (QuickJS running the components), so [`Renderer::run`]
//! runs the closure on tokio's blocking thread pool. The [`Session`] gives its instance back to
//! the pool when it's dropped, or replaces it if a call failed.

use std::{path::PathBuf, sync::Arc, time::Duration};

use anyhow::Context as _;
use extism::{CompiledPlugin, Manifest, Plugin, PluginBuilder, Wasm};
use serde::Deserialize;
use serde_json::json;
use tokio::sync::{Mutex, mpsc};

/// Bounds a single plugin call.
const CALL_TIMEOUT: Duration = Duration::from_secs(30);
/// How long a request waits for a free instance.
const WAIT_TIMEOUT: Duration = Duration::from_secs(30);

/// The compiled plugin and its pool of instances. Cheap to clone.
#[derive(Clone)]
pub struct Renderer(Arc<Pool>);

struct Pool {
    compiled: CompiledPlugin,
    idle_tx: mpsc::Sender<Plugin>,
    idle_rx: Mutex<mpsc::Receiver<Plugin>>,
}

impl Renderer {
    /// Compiles the plugin and creates `pool_size` instances. Blocks for a few seconds without a
    /// warm cache, so call it before starting the async runtime.
    pub fn new(wasm_path: PathBuf, pool_size: usize, cache_config: Option<PathBuf>) -> anyhow::Result<Self> {
        let manifest = Manifest::new([Wasm::file(&wasm_path)]).with_timeout(CALL_TIMEOUT);
        let builder = PluginBuilder::new(manifest).with_wasi(true);
        // Without either call, Extism uses a cache in ~/.cache/wasmtime and fails if it can't
        // create it.
        let builder = match &cache_config {
            Some(path) => builder.with_cache_config(path),
            None => builder.with_cache_disabled(),
        };
        let compiled = CompiledPlugin::new(builder)
            .with_context(|| format!("ssr: compile {}", wasm_path.display()))?;

        let pool_size = pool_size.max(1);
        let (idle_tx, idle_rx) = mpsc::channel(pool_size);
        let pool = Pool { compiled, idle_tx, idle_rx: Mutex::new(idle_rx) };
        for _ in 0..pool_size {
            // The channel holds `pool_size` instances, so sending never fails.
            let _ = pool.idle_tx.try_send(pool.new_instance()?);
        }
        Ok(Self(Arc::new(pool)))
    }

    /// Waits (asynchronously, up to 30 s) for a free instance, then runs `f` with it on the
    /// blocking thread pool.
    pub async fn run<R, F>(&self, f: F) -> anyhow::Result<R>
    where
        F: FnOnce(Session) -> R + Send + 'static,
        R: Send + 'static,
    {
        let take = async { self.0.idle_rx.lock().await.recv().await };
        let plugin = tokio::time::timeout(WAIT_TIMEOUT, take)
            .await
            .ok()
            .flatten()
            .context("ssr: no plugin instance free within 30 s")?;
        let session = Session { pool: self.0.clone(), plugin: Some(plugin), reset: false, broken: false };
        // If the client goes away, `f` still runs to completion and the instance goes back.
        tokio::task::spawn_blocking(move || f(session)).await.context("ssr: render panicked")
    }
}

impl Pool {
    fn new_instance(&self) -> anyhow::Result<Plugin> {
        let mut p = Plugin::new_from_compiled(&self.compiled).context("ssr: instantiate")?;
        // The first call starts QuickJS and loads the components; do it now, not on a request.
        p.call::<&str, &[u8]>("resetSsrDocData", "").context("ssr: warm up instance")?;
        Ok(p)
    }
}

/// One request's exclusive use of a plugin instance.
pub struct Session {
    pool: Arc<Pool>,
    plugin: Option<Plugin>, // taken in `drop`
    /// `resetSsrDocData` has been called for this request.
    reset: bool,
    /// A call failed (trap, timeout), so the instance can't be trusted.
    broken: bool,
}

impl std::fmt::Debug for Session {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Session").field("reset", &self.reset).field("broken", &self.broken).finish()
    }
}

impl Session {
    /// Renders a whole document (default options). Hydration ids start from 1 by themselves.
    pub fn render_document(&mut self, html: &str) -> String {
        self.render(html, false)
    }

    /// Renders a fragment (`{"fullDocument": false}`). The first fragment of the session calls
    /// `resetSsrDocData`, so ids start from 1 per request and never collide within it.
    pub fn render_fragment(&mut self, html: &str) -> String {
        self.render(html, true)
    }

    /// On any error (`__error` or a failed call) logs and returns `html`, so the components still
    /// render in the browser.
    fn render(&mut self, html: &str, fragment: bool) -> String {
        match self.try_render(html, fragment) {
            Ok(out) => out,
            Err(e) => {
                tracing::error!("ssr: serving unrendered HTML: {e:#}");
                html.to_owned()
            }
        }
    }

    fn try_render(&mut self, html: &str, fragment: bool) -> anyhow::Result<String> {
        anyhow::ensure!(!self.broken, "the plugin instance failed earlier in this request");
        if fragment && !self.reset {
            self.call("resetSsrDocData", "")?;
            self.reset = true;
        }
        let options = if fragment { json!({ "fullDocument": false }) } else { json!({}) };
        let out = self.call("renderToString", &json!({ "html": html, "options": options }).to_string())?;
        parse_result(&out)
    }

    fn call(&mut self, function: &str, input: &str) -> anyhow::Result<String> {
        let plugin = self.plugin.as_mut().expect("plugin is only taken on drop");
        let out = plugin.call::<&str, &str>(function, input).map(str::to_owned);
        self.broken |= out.is_err();
        out
    }
}

impl Drop for Session {
    /// Returns the instance to the pool, or replaces it if a call failed or the render panicked.
    fn drop(&mut self) {
        let Some(old) = self.plugin.take() else { return };
        let plugin = if self.broken || std::thread::panicking() {
            drop(old);
            match self.pool.new_instance() {
                Ok(p) => p,
                Err(e) => {
                    // The pool stays one smaller.
                    tracing::error!("ssr: replacing a broken plugin instance failed: {e:#}");
                    return;
                }
            }
        } else {
            old
        };
        let _ = self.pool.idle_tx.try_send(plugin);
    }
}

/// Extracts the HTML from `renderToString`'s JSON result, or fails with the plugin's `__error`.
fn parse_result(out: &str) -> anyhow::Result<String> {
    #[derive(Deserialize)]
    struct RenderResult {
        html: Option<String>,
        #[serde(rename = "__error")]
        error: Option<serde_json::Value>,
    }
    let result: RenderResult = serde_json::from_str(out).context("decoding result")?;
    if let Some(err) = result.error {
        anyhow::bail!("__error: {err}");
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
                let path = std::env::var("SSR_WASM_PATH")
                    .unwrap_or_else(|_| "../../packages/components/dist/ssr-wasm/index.wasm".into());
                if !std::path::Path::new(&path).exists() {
                    eprintln!("plugin not found at {path} (set SSR_WASM_PATH); skipping");
                    return None;
                }
                let cache = std::env::var_os("SSR_CACHE_CONFIG").map(PathBuf::from);
                Some(Renderer::new(path.into(), 2, cache).expect("renderer"))
            })
            .clone()
    }

    fn ids(html: &str) -> Vec<u32> {
        html.split(" s-id=\"").skip(1).map(|s| s[..s.find('"').unwrap()].parse().unwrap()).collect()
    }

    #[tokio::test]
    async fn fragment_render() {
        let Some(r) = renderer() else { return };
        let out = r
            .run(|mut s| s.render_fragment(r#"<demo-counter start="5" step="2"></demo-counter>"#))
            .await
            .unwrap();
        assert!(out.contains(r#"<template shadowrootmode="open">"#), "{out}");
        assert!(!out.contains("<html"), "a fragment render shouldn't produce a document: {out}");
    }

    /// Fragments in one session continue the id sequence; the next session starts again from 1.
    #[tokio::test]
    async fn fragment_ids_reset_once_per_session() {
        let Some(r) = renderer() else { return };
        let frag = r#"<demo-badge tone="success">SSR</demo-badge>"#;
        for _ in 0..3 {
            let (a, b) = r
                .run(move |mut s| (ids(&s.render_fragment(frag)), ids(&s.render_fragment(frag))))
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
            .run(move |mut s| (0..3).map(|_| ids(&s.render_document(page))).collect::<Vec<_>>())
            .await
            .unwrap();
        assert!(all.iter().all(|ids| ids == &[1, 2]), "{all:?}");
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
