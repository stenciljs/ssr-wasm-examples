//! The two pages, rendered with MiniJinja, and the `stencil` template filter used by helper mode.

use std::sync::{Arc, Mutex};

use axum::{
    extract::State,
    http::StatusCode,
    response::{Html, IntoResponse, Response},
};
use minijinja::{Environment, Error, ErrorKind, State as TemplateState, Value, context, value::Object};
use serde::Serialize;

use crate::{AppState, ssr::Session};

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
    /// Passed to the templates as a plain (not safe) string, so autoescaping escapes it for the
    /// attribute it's written into.
    tasks_json: String,
}

impl Pages {
    pub fn new() -> anyhow::Result<Self> {
        let mut env = Environment::new();
        // Names ending in .html get HTML autoescaping.
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
        Ok(Self { env, tasks_json: serde_json::to_string(TASKS)? })
    }

    fn render(&self, name: &str, ssr: Option<Value>) -> Result<String, Error> {
        self.env.get_template(name)?.render(context! {
            framework => "Rust Axum",
            tasks => self.tasks_json.as_str(),
            ssr => ssr,
        })
    }
}

/// The request's plugin session, visible to the template as `ssr`. Every `stencil` filter in one
/// render uses it, so they share one instance and one hydration-id sequence.
#[derive(Debug)]
struct SsrSession(Mutex<Session>);

impl Object for SsrSession {}

/// `{% filter stencil %}<demo-card ...>...</demo-card>{% endfilter %}`
///
/// MiniJinja renders the block body with normal autoescaping; the filter renders that markup as
/// a fragment and returns it as a safe string, so it's inlined as-is.
fn stencil(state: &TemplateState, html: String) -> Result<Value, Error> {
    let ssr = state.lookup("ssr");
    let session = ssr
        .as_ref()
        .and_then(|v| v.downcast_object_ref::<SsrSession>())
        .ok_or_else(|| Error::new(ErrorKind::InvalidOperation, "stencil: no `ssr` session in this render"))?;
    let rendered = session.0.lock().unwrap().render_fragment(&html);
    Ok(Value::from_safe_string(rendered))
}

/// `GET /`: plain markup; the middleware around this route renders it.
pub async fn index(State(app): State<AppState>) -> Response {
    match app.pages.render("index.html", None) {
        Ok(html) => Html(html).into_response(),
        Err(e) => template_error(e),
    }
}

/// `GET /helper`: takes one plugin instance and renders the whole template with it, on a
/// blocking thread. The session (and its instance) is released when the render finishes.
pub async fn helper(State(app): State<AppState>) -> Response {
    let pages = app.pages.clone();
    let result = app
        .renderer
        .run(move |session| {
            let ssr = Value::from_dyn_object(Arc::new(SsrSession(Mutex::new(session))));
            pages.render("helper.html", Some(ssr))
        })
        .await;
    match result {
        Ok(Ok(html)) => Html(html).into_response(),
        Ok(Err(e)) => template_error(e),
        Err(e) => {
            tracing::error!("{e:#}");
            StatusCode::SERVICE_UNAVAILABLE.into_response()
        }
    }
}

fn template_error(e: Error) -> Response {
    tracing::error!("template: {e:#}");
    StatusCode::INTERNAL_SERVER_ERROR.into_response()
}
