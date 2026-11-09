//! Serves the ssr-wasm example pages with Axum and the Extism Rust SDK (wasmtime).
//!
//! ```text
//! GET /         middleware mode: ssr_middleware renders the whole HTML response
//! GET /helper   helper mode: the template renders each component through {% filter stencil %}
//! GET /health   "ok", without touching the plugin
//! GET /stencil/ the client bundle
//! ```
//!
//! Two flags for the Docker image: `--warm-cache` compiles the plugin into the wasmtime cache and
//! exits (at build time), and `--healthcheck` is the HEALTHCHECK (distroless has no curl).

mod middleware;
mod pages;
mod ssr;

use std::{
    io::{Read, Write},
    net::{Ipv4Addr, SocketAddr, TcpStream},
    path::PathBuf,
    sync::Arc,
    time::Duration,
};

use anyhow::Context as _;
use axum::{Router, middleware::from_fn_with_state, routing::get};
use tower_http::services::ServeDir;

#[derive(Clone)]
pub struct AppState {
    pub renderer: ssr::Renderer,
    pub pages: Arc<pages::Pages>,
}

fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env().unwrap_or_else(|_| "info".into()),
        )
        .init();
    let flag = |name: &str| std::env::args().any(|a| a == name);
    let port: u16 = env("PORT", "8005").parse().context("PORT")?;
    if flag("--healthcheck") {
        std::process::exit(if healthcheck(port) { 0 } else { 1 });
    }

    // Compile the plugin and fill the pool before serving (blocking, a few seconds uncached).
    let wasm_path = env("SSR_WASM_PATH", "../../packages/components/dist/ssr-wasm/index.wasm");
    let pool_size = std::env::var("SSR_POOL_SIZE")
        .ok()
        .and_then(|n| n.parse::<usize>().ok())
        .unwrap_or_else(|| std::thread::available_parallelism().map_or(1, usize::from));
    let cache_config = std::env::var_os("SSR_CACHE_CONFIG").filter(|v| !v.is_empty()).map(PathBuf::from);
    let warm_cache = flag("--warm-cache");
    let renderer = ssr::Renderer::new(
        PathBuf::from(&wasm_path),
        if warm_cache { 1 } else { pool_size },
        cache_config,
    )?;
    tracing::info!(wasm = %wasm_path, pool_size, "ssr-wasm plugin ready");
    if warm_cache {
        return Ok(());
    }

    let client_dir = env("STENCIL_CLIENT_DIR", "../../packages/components/dist/loader-bundle/demo-components");
    let state = AppState { renderer: renderer.clone(), pages: Arc::new(pages::Pages::new()?) };
    // Only `/` goes through the middleware; `/helper` renders its own fragments.
    let middleware_mode = Router::new()
        .route("/", get(pages::index))
        .route_layer(from_fn_with_state(renderer, middleware::ssr_middleware));
    let app = Router::new()
        .merge(middleware_mode)
        .route("/helper", get(pages::helper))
        .route("/health", get(|| async { "ok" }))
        .nest_service("/stencil", ServeDir::new(client_dir))
        .with_state(state);

    tokio::runtime::Builder::new_multi_thread().enable_all().build()?.block_on(async move {
        let listener = tokio::net::TcpListener::bind((Ipv4Addr::UNSPECIFIED, port)).await?;
        tracing::info!("listening on http://localhost:{port}");
        axum::serve(listener, app).with_graceful_shutdown(shutdown_signal()).await?;
        Ok::<_, anyhow::Error>(())
    })
}

/// Ctrl-C or SIGTERM (`docker stop`: as PID 1, the server would otherwise ignore it).
async fn shutdown_signal() {
    #[cfg(unix)]
    {
        use tokio::signal::unix::{SignalKind, signal};
        let mut term = signal(SignalKind::terminate()).expect("SIGTERM handler");
        tokio::select! { _ = tokio::signal::ctrl_c() => {}, _ = term.recv() => {} }
    }
    #[cfg(not(unix))]
    let _ = tokio::signal::ctrl_c().await;
}

/// `GET /health` over a plain socket: true on a 200.
fn healthcheck(port: u16) -> bool {
    let check = || -> std::io::Result<bool> {
        let addr = SocketAddr::from((Ipv4Addr::LOCALHOST, port));
        let mut s = TcpStream::connect_timeout(&addr, Duration::from_secs(3))?;
        s.set_read_timeout(Some(Duration::from_secs(3)))?;
        s.write_all(b"GET /health HTTP/1.0\r\nHost: localhost\r\n\r\n")?;
        let mut status_line = [0u8; 12]; // "HTTP/1.1 200"
        s.read_exact(&mut status_line)?;
        Ok(&status_line[9..] == b"200")
    };
    check().unwrap_or(false)
}

fn env(key: &str, default: &str) -> String {
    std::env::var(key).ok().filter(|v| !v.is_empty()).unwrap_or_else(|| default.to_owned())
}
