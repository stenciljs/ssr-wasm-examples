//! Serves the ssr-wasm example pages with Axum and the Extism Rust SDK (wasmtime).
//!
//! ```text
//! GET /         middleware mode: ssr_middleware renders the whole HTML response
//! GET /helper   helper mode: the template renders each component through {% filter stencil %}
//! GET /health   "ok", without touching the plugin
//! GET /stencil/ the client bundle
//! ```
//!
//! `server --warm-cache` compiles the plugin into the wasmtime cache and exits (used by the
//! Dockerfile); `server --healthcheck` is the container HEALTHCHECK.

mod middleware;
mod pages;
mod ssr;

use std::{
    io::{Read, Write},
    net::{Ipv4Addr, SocketAddr, TcpStream},
    path::PathBuf,
    process::ExitCode,
    sync::Arc,
    time::Duration,
};

use anyhow::Context as _;
use axum::{Router, middleware::from_fn_with_state, routing::get};
use tower_http::services::ServeDir;

/// Shared by every handler. Cheap to clone.
#[derive(Clone)]
pub struct AppState {
    pub renderer: ssr::Renderer,
    pub pages: Arc<pages::Pages>,
}

fn main() -> ExitCode {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env().unwrap_or_else(|_| "info".into()),
        )
        // No colour codes in `docker logs` or a log file.
        .with_ansi(std::io::IsTerminal::is_terminal(&std::io::stderr()))
        .with_writer(std::io::stderr)
        .init();
    match run() {
        Ok(code) => code,
        Err(e) => {
            tracing::error!("{e:#}");
            ExitCode::FAILURE
        }
    }
}

fn run() -> anyhow::Result<ExitCode> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let flag = |name: &str| args.iter().any(|a| a == name);
    let port: u16 = env("PORT", "8005").parse().context("PORT")?;

    if flag("--healthcheck") {
        return Ok(healthcheck(port));
    }
    let warm_cache = flag("--warm-cache");

    let mut cfg = ssr::Config::new(env(
        "SSR_WASM_PATH",
        "../../packages/components/dist/ssr-wasm/index.wasm",
    ));
    if let Ok(n) = std::env::var("SSR_POOL_SIZE") {
        cfg.pool_size = n.parse().ok().filter(|&n| n > 0).context("SSR_POOL_SIZE must be a positive integer")?;
    }
    cfg.cache_config = std::env::var_os("SSR_CACHE_CONFIG").filter(|v| !v.is_empty()).map(PathBuf::from);
    if warm_cache {
        anyhow::ensure!(cfg.cache_config.is_some(), "--warm-cache needs SSR_CACHE_CONFIG");
        cfg.pool_size = 1;
    }
    let client_dir = PathBuf::from(env(
        "STENCIL_CLIENT_DIR",
        "../../packages/components/dist/loader-bundle/demo-components",
    ));

    // Blocking, and done before the async runtime exists: there's nothing to serve until the pool
    // is ready anyway.
    let renderer = ssr::Renderer::new(&cfg)?;
    tracing::info!(
        wasm = %cfg.wasm_path.display(),
        compile = ?round(renderer.stats.compile),
        instantiate = ?renderer.stats.instantiate.iter().copied().map(round).collect::<Vec<_>>(),
        pool = cfg.pool_size,
        cache = ?cfg.cache_config,
        "ssr-wasm plugin ready"
    );
    if warm_cache {
        return Ok(ExitCode::SUCCESS);
    }

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
        let addr = SocketAddr::from((Ipv4Addr::UNSPECIFIED, port));
        let listener = tokio::net::TcpListener::bind(addr).await.with_context(|| format!("bind {addr}"))?;
        tracing::info!("listening on http://localhost:{port}");
        axum::serve(listener, app).with_graceful_shutdown(shutdown_signal()).await?;
        Ok::<_, anyhow::Error>(ExitCode::SUCCESS)
    })
}

/// Ctrl-C or SIGTERM (`docker stop`; as PID 1 the default action would be ignored).
async fn shutdown_signal() {
    let ctrl_c = async {
        let _ = tokio::signal::ctrl_c().await;
    };
    #[cfg(unix)]
    let term = async {
        match tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate()) {
            Ok(mut s) => {
                s.recv().await;
            }
            Err(_) => std::future::pending().await,
        }
    };
    #[cfg(not(unix))]
    let term = std::future::pending::<()>();
    tokio::select! { _ = ctrl_c => {}, _ = term => {} }
    tracing::info!("shutting down");
}

/// The container HEALTHCHECK: the runtime image has no curl or shell.
fn healthcheck(port: u16) -> ExitCode {
    let check = || -> std::io::Result<bool> {
        let addr = SocketAddr::from((Ipv4Addr::LOCALHOST, port));
        let mut s = TcpStream::connect_timeout(&addr, Duration::from_secs(3))?;
        s.set_read_timeout(Some(Duration::from_secs(3)))?;
        s.write_all(b"GET /health HTTP/1.0\r\nHost: localhost\r\n\r\n")?;
        let mut buf = [0u8; 16];
        let n = s.read(&mut buf)?;
        let status_line = String::from_utf8_lossy(&buf[..n]);
        Ok(status_line.starts_with("HTTP/1.1 200") || status_line.starts_with("HTTP/1.0 200"))
    };
    match check() {
        Ok(true) => ExitCode::SUCCESS,
        Ok(false) => ExitCode::FAILURE,
        Err(e) => {
            eprintln!("healthcheck: {e}");
            ExitCode::FAILURE
        }
    }
}

fn env(key: &str, default: &str) -> String {
    std::env::var(key).ok().filter(|v| !v.is_empty()).unwrap_or_else(|| default.to_owned())
}

/// To 0.1 ms, for logging.
fn round(d: Duration) -> Duration {
    Duration::from_micros(d.as_micros() as u64 / 100 * 100)
}
