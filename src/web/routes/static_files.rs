//! Static file serving for the embedded frontend.

use axum::{
    body::Body,
    extract::Path,
    http::{header, Response, StatusCode},
    response::IntoResponse,
};
use rust_embed::RustEmbed;

/// Embedded frontend assets from web/dist.
#[derive(RustEmbed)]
#[folder = "web/dist"]
struct FrontendAssets;

/// Serve a static file from the embedded assets.
pub async fn serve_static_file(Path(path): Path<String>) -> impl IntoResponse {
    // The route strips /assets/ prefix, so we need to add it back
    let full_path = format!("assets/{}", path);
    serve_file(&full_path)
}

/// Serve the index.html for SPA routing (catch-all for unmatched routes).
pub async fn serve_index() -> impl IntoResponse {
    serve_file("index.html")
}

/// Internal function to serve a file from embedded assets.
fn serve_file(path: &str) -> Response<Body> {
    match FrontendAssets::get(path) {
        Some(content) => {
            let mime = mime_guess::from_path(path).first_or_octet_stream();
            Response::builder()
                .status(StatusCode::OK)
                .header(header::CONTENT_TYPE, mime.as_ref())
                .header(header::CACHE_CONTROL, cache_control(path))
                .body(Body::from(content.data.into_owned()))
                .unwrap()
        }
        None => {
            // For SPA routing, serve index.html for HTML requests
            if !path.contains('.') {
                if let Some(index) = FrontendAssets::get("index.html") {
                    return Response::builder()
                        .status(StatusCode::OK)
                        .header(header::CONTENT_TYPE, "text/html")
                        .body(Body::from(index.data.into_owned()))
                        .unwrap();
                }
            }
            Response::builder()
                .status(StatusCode::NOT_FOUND)
                .body(Body::from("Not Found"))
                .unwrap()
        }
    }
}

/// Determine cache control based on file type.
fn cache_control(path: &str) -> &'static str {
    // Cache assets with hashes for a long time
    if path.starts_with("assets/") {
        "public, max-age=31536000, immutable"
    } else if path == "index.html" {
        // Don't cache index.html to ensure users get updates
        "no-cache, no-store, must-revalidate"
    } else {
        // Default caching for other files
        "public, max-age=3600"
    }
}
