//! The built web UI (`web/dist`) embedded into the binary.
//!
//! In debug builds `rust-embed` reads from disk at runtime, so `npm run build`
//! in `web/` is picked up without recompiling. Release builds embed the files.

use axum::{
    body::Body,
    http::{HeaderValue, StatusCode, Uri, header},
    response::{IntoResponse, Response},
};
use rust_embed::Embed;

#[derive(Embed)]
#[folder = "../../web/dist/"]
struct WebDist;

/// Serve a static file if it exists, otherwise `index.html` (SPA fallback).
pub async fn spa_fallback(uri: Uri) -> Response {
    let path = uri.path().trim_start_matches('/');
    // Unknown API routes are an error, not the app shell.
    if path.starts_with("api/") || path == "api" {
        return (StatusCode::NOT_FOUND, "no such API route").into_response();
    }
    if !path.is_empty()
        && let Some(file) = WebDist::get(path)
    {
        return file_response(path, file.data.into_owned(), true);
    }
    match WebDist::get("index.html") {
        Some(index) => file_response("index.html", index.data.into_owned(), false),
        None => (
            StatusCode::NOT_FOUND,
            "web UI not built: run `npm run build` in web/",
        )
            .into_response(),
    }
}

fn file_response(path: &str, data: Vec<u8>, immutable: bool) -> Response {
    let mime = mime_guess::from_path(path).first_or_octet_stream();
    let cache = if immutable && path.starts_with("assets/") {
        "public, max-age=31536000, immutable"
    } else {
        "no-cache"
    };
    let mut res = Response::new(Body::from(data));
    res.headers_mut().insert(
        header::CONTENT_TYPE,
        HeaderValue::from_str(mime.as_ref())
            .unwrap_or(HeaderValue::from_static("application/octet-stream")),
    );
    res.headers_mut()
        .insert(header::CACHE_CONTROL, HeaderValue::from_static(cache));
    res
}
