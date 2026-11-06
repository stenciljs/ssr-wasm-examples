//! Middleware mode: server-render the Stencil components in whole `text/html` responses.

use std::{sync::Arc, time::Duration};

use axum::{
    body::{Body, to_bytes},
    extract::{Request, State},
    http::{HeaderValue, StatusCode, header},
    middleware::Next,
    response::{IntoResponse, Response},
};

use crate::ssr::{Options, Renderer};

/// Pages bigger than this aren't something you'd want to render in one go.
const MAX_HTML_BYTES: usize = 16 * 1024 * 1024;

/// An `axum::middleware::from_fn_with_state` middleware. It buffers the wrapped handler's
/// response, passes the whole document through `renderToString` (default options,
/// `fullDocument: true`) and returns the result. Anything else - other content types, encoded
/// bodies, bodies with no content - passes through untouched.
pub async fn ssr_middleware(State(renderer): State<Renderer>, req: Request, next: Next) -> Response {
    let res = next.run(req).await;
    if !is_renderable(&res) {
        return res;
    }

    let (mut parts, body) = res.into_parts();
    let bytes = match to_bytes(body, MAX_HTML_BYTES).await {
        Ok(b) => b,
        Err(e) => {
            tracing::error!("ssr middleware: reading the response body: {e}");
            return (StatusCode::INTERNAL_SERVER_ERROR, "internal server error").into_response();
        }
    };
    let html: Arc<str> = match std::str::from_utf8(&bytes) {
        Ok(s) => s.into(),
        Err(_) => return Response::from_parts(parts, Body::from(bytes)), // not UTF-8: leave it be
    };

    let input = html.clone();
    let rendered = renderer
        .run(move |mut h| {
            let out = h.render(&input, &Options::new());
            (out, h.elapsed())
        })
        .await;
    let body = match rendered {
        Ok((out, elapsed)) => {
            if let Ok(v) = HeaderValue::from_str(&server_timing(elapsed)) {
                parts.headers.insert("server-timing", v);
            }
            out
        }
        Err(e) => {
            tracing::error!("ssr middleware: {e}; serving unrendered HTML");
            html.to_string()
        }
    };
    parts.headers.remove(header::CONTENT_LENGTH); // the rendered body has a different length
    Response::from_parts(parts, Body::from(body))
}

fn is_renderable(res: &Response) -> bool {
    let status = res.status();
    if status.is_informational() || status == StatusCode::NO_CONTENT || status == StatusCode::NOT_MODIFIED {
        return false;
    }
    let h = res.headers();
    let is_html = h
        .get(header::CONTENT_TYPE)
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.split(';').next())
        .is_some_and(|mt| mt.trim().eq_ignore_ascii_case("text/html"));
    is_html && !h.contains_key(header::CONTENT_ENCODING)
}

/// A `Server-Timing` value for the time spent in the plugin.
pub fn server_timing(d: Duration) -> String {
    format!("ssr;desc=\"Stencil SSR WASM\";dur={:.2}", d.as_secs_f64() * 1000.0)
}
