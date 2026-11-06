//! The two pages, rendered with MiniJinja, and the `stencil` template filter used by helper mode.

use std::{
    sync::{Arc, Mutex},
    time::Duration,
};

use axum::{
    extract::State,
    http::{HeaderValue, StatusCode},
    response::{Html, IntoResponse, Response},
};
use minijinja::{Environment, Error, ErrorKind, State as TemplateState, Value, context, value::Object};
use serde::Serialize;

use crate::{
    AppState,
    middleware::server_timing,
    ssr::{self, Handle},
};

pub const FRAMEWORK: &str = "Rust Axum";

#[derive(Serialize)]
struct Task {
    title: &'static str,
    #[serde(skip_serializing_if = "Option::is_none")]
    done: Option<bool>, // the third task has no "done" key at all
}

const TASKS: &[Task] = &[
    Task { title: "Write components", done: Some(true) },
    Task { title: r#"Render with <wasm> & "friends""#, done: Some(false) },
    Task { title: "Hydrate on the client", done: None },
];

/// The templates, embedded in the binary and parsed once at startup.
pub struct Pages {
    env: Environment<'static>,
    /// The task list as JSON. It's passed to the templates as a plain string, not a safe one,
    /// so MiniJinja's autoescaping escapes it for the attribute it's written into.
    tasks_json: String,
}

impl Pages {
    pub fn new() -> anyhow::Result<Self> {
        let mut env = Environment::new();
        // Names ending in .html get HTML autoescaping (MiniJinja's default callback).
        for (name, source) in [
            ("layout.html", include_str!("../templates/layout.html")),
            ("index.html", include_str!("../templates/index.html")),
            ("helper.html", include_str!("../templates/helper.html")),
            ("components/demo-notice.html", include_str!("../templates/components/demo-notice.html")),
            ("components/demo-card.html", include_str!("../templates/components/demo-card.html")),
            ("components/demo-counter.html", include_str!("../templates/components/demo-counter.html")),
            ("components/demo-task-list.html", include_str!("../templates/components/demo-task-list.html")),
        ] {
            env.add_template(name, source)?;
        }
        env.add_filter("stencil", stencil);
        // A typo'd variable is an error, not an empty string.
        env.set_undefined_behavior(minijinja::UndefinedBehavior::Strict);
        Ok(Self { env, tasks_json: serde_json::to_string(TASKS)? })
    }

    fn render(&self, name: &str, ssr: Option<Value>) -> Result<String, Error> {
        self.env.get_template(name)?.render(context! {
            framework => FRAMEWORK,
            tasks => self.tasks_json.as_str(),
            ssr => ssr,
        })
    }

    /// Renders the helper page with `handle`, returning the page and the time spent in the plugin.
    /// Runs on a blocking thread (inside [`ssr::Renderer::run`]).
    fn render_helper(&self, handle: Handle) -> Result<(String, Duration), Error> {
        let ssr = Arc::new(SsrContext(Mutex::new(handle)));
        let html = self.render("helper.html", Some(Value::from_dyn_object(ssr.clone())))?;
        let elapsed = ssr.lock().elapsed();
        Ok((html, elapsed))
        // `ssr` drops here (the template's references are gone), returning the instance.
    }
}

/// The request's plugin handle, made visible to templates as the `ssr` variable. Every `stencil`
/// call in one render shares it, and so shares one instance and one hydration-id sequence.
#[derive(Debug)]
struct SsrContext(Mutex<Handle>);

impl SsrContext {
    fn lock(&self) -> std::sync::MutexGuard<'_, Handle> {
        self.0.lock().unwrap_or_else(|e| e.into_inner())
    }
}

impl Object for SsrContext {}

/// `{% filter stencil %}<demo-card ...>...</demo-card>{% endfilter %}`
///
/// The block body is rendered first with normal autoescaping, then this filter passes that
/// markup to `renderToString` with `{"fullDocument": false}` using the request's handle, and
/// returns the result as a safe string so it's inlined as-is.
fn stencil(state: &TemplateState, html: String) -> Result<Value, Error> {
    let ssr = state
        .lookup("ssr")
        .filter(|v| !v.is_none())
        .ok_or_else(|| Error::new(ErrorKind::InvalidOperation, "stencil: no `ssr` handle in this render"))?;
    let ssr = ssr
        .downcast_object_ref::<SsrContext>()
        .ok_or_else(|| Error::new(ErrorKind::InvalidOperation, "stencil: `ssr` is not a plugin handle"))?;
    Ok(Value::from_safe_string(ssr.lock().render(&html, &ssr::fragment())))
}

/// `GET /`: renders the page with no SSR; the middleware wrapped around it does that.
pub async fn index(State(app): State<AppState>) -> Response {
    match app.pages.render("index.html", None) {
        Ok(html) => Html(html).into_response(),
        Err(e) => template_error(e),
    }
}

/// `GET /helper`: takes one plugin instance for the request and renders the template with it,
/// on a blocking thread. Every `stencil` filter in the template shares that instance (and its
/// hydration-id sequence, reset before the first fragment).
pub async fn helper(State(app): State<AppState>) -> Response {
    let pages = app.pages.clone();
    match app.renderer.run(move |h| pages.render_helper(h)).await {
        Ok(Ok((html, elapsed))) => {
            let mut res = Html(html).into_response();
            if let Ok(v) = HeaderValue::from_str(&server_timing(elapsed)) {
                res.headers_mut().insert("server-timing", v);
            }
            res
        }
        Ok(Err(e)) => template_error(e),
        Err(e) => {
            tracing::error!("helper: {e}");
            (StatusCode::SERVICE_UNAVAILABLE, "service unavailable").into_response()
        }
    }
}

fn template_error(e: Error) -> Response {
    tracing::error!("template: {e:#}");
    (StatusCode::INTERNAL_SERVER_ERROR, "internal server error").into_response()
}
