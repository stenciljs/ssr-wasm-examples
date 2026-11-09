//! Middleware mode: server-render the whole `text/html` response.

use axum::{
    body::{Body, to_bytes},
    extract::{Request, State},
    http::{StatusCode, header},
    middleware::Next,
    response::{IntoResponse, Response},
};

use crate::ssr::Renderer;

/// An `axum::middleware::from_fn_with_state` middleware. It buffers the handler's HTML response
/// and passes it through `renderToString` (default options, `fullDocument: true`). Other
/// responses pass through untouched.
pub async fn ssr_middleware(State(renderer): State<Renderer>, req: Request, next: Next) -> Response {
    let res = next.run(req).await;
    if !is_html(&res) {
        return res;
    }

    let (mut parts, body) = res.into_parts();
    let html = match to_bytes(body, usize::MAX).await {
        Ok(bytes) => String::from_utf8_lossy(&bytes).into_owned(),
        Err(e) => {
            tracing::error!("ssr middleware: reading the response body: {e}");
            return StatusCode::INTERNAL_SERVER_ERROR.into_response();
        }
    };
    match renderer.run(move |mut s| s.render_document(&html)).await {
        Ok(rendered) => {
            parts.headers.remove(header::CONTENT_LENGTH); // the rendered body is longer
            Response::from_parts(parts, Body::from(rendered))
        }
        Err(e) => {
            tracing::error!("{e:#}");
            StatusCode::SERVICE_UNAVAILABLE.into_response()
        }
    }
}

/// Uncompressed `text/html` responses.
fn is_html(res: &Response) -> bool {
    let h = res.headers();
    let content_type = h.get(header::CONTENT_TYPE).and_then(|v| v.to_str().ok()).unwrap_or("");
    content_type.starts_with("text/html") && !h.contains_key(header::CONTENT_ENCODING)
}
