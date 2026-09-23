//! Embedded web UI for inspecting and administrating the Ora scheduler.
//!
//! The UI is a single page application that talks to the Ora admin API via gRPC-web,
//! this crate embeds the built assets and serves them with [`axum`].
//!
//! # Example
//!
//! ```no_run
//! # fn example(grpc: axum::Router) {
//! // `grpc` is a router serving the Ora admin service with gRPC-web support,
//! // e.g. `tonic::service::Routes::new(..).into_axum_router()`
//! // with a `tonic_web::GrpcWebLayer`.
//! let app = grpc.nest("/ui", ora_ui::router(ora_ui::UiOptions::default()));
//! # }
//! ```
//!
//! Note that nested routers do not match the path with a trailing slash (e.g. `/ui/`),
//! add a redirect for it if needed:
//!
//! ```no_run
//! # fn example(app: axum::Router) -> axum::Router {
//! app.route(
//!     "/ui/",
//!     axum::routing::get(|| async { axum::response::Redirect::permanent("/ui") }),
//! )
//! # }
//! ```
//!
//! By default the UI expects the API on the same origin, see [`UiOptions::api_url`]
//! if it is served from elsewhere.

use axum::{
    Router,
    body::Body,
    extract::{OriginalUri, State},
    http::{HeaderValue, StatusCode, Uri, header},
    response::{IntoResponse, Response},
    routing::get,
};
use include_dir::{Dir, include_dir};
use std::sync::Arc;

// The build script ensures that the assets exist.
static ASSETS: Dir<'static> = include_dir!("$CARGO_MANIFEST_DIR/dist");

include!("markers.rs");

/// Options for serving the UI.
#[derive(Debug, Clone, Default)]
pub struct UiOptions {
    /// The base URL of the Ora admin API with gRPC-web support
    /// as seen from the browser, e.g. `https://ora.example.com`.
    ///
    /// If not set, the API is expected to be served on the same origin as the UI.
    pub api_url: Option<String>,
}

struct UiState {
    /// `index.html` with the API URL already configured.
    index: String,
}

/// Returns a router that serves the UI.
///
/// The router can be nested under any path, e.g. `/ui`,
/// or merged at the root of an application.
/// When nested, the path with a trailing slash (e.g. `/ui/`) is not matched,
/// see the [crate documentation](crate) for handling it. All paths that do not
/// match an asset serve the UI's `index.html`, so that client-side routes work on reload.
///
/// Responses are not compressed, add a compression layer
/// (e.g. `tower_http::compression::CompressionLayer`) if needed.
pub fn router<S>(options: UiOptions) -> Router<S>
where
    S: Clone + Send + Sync + 'static,
{
    let index = ASSETS
        .get_file("index.html")
        .and_then(|file| file.contents_utf8())
        .unwrap_or_default();

    let index = index.replace(
        API_URL_MARKER,
        &format!(
            r#"<meta name="ora-api-url" content="{}" />"#,
            escape_attribute(options.api_url.as_deref().unwrap_or_default())
        ),
    );

    let state = Arc::new(UiState { index });

    Router::new()
        .route("/", get(serve))
        .route("/{*path}", get(serve))
        .with_state(state)
}

async fn serve(
    State(state): State<Arc<UiState>>,
    uri: Uri,
    OriginalUri(original_uri): OriginalUri,
) -> Response {
    let path = uri.path();
    let original_path = original_uri.path();

    // The path prefix the router is nested under (if any).
    let prefix = original_path.strip_suffix(path).unwrap_or(original_path);

    let asset_path = path.trim_start_matches('/');

    if !asset_path.is_empty() && asset_path != "index.html" {
        if let Some(file) = ASSETS.get_file(asset_path) {
            return asset_response(asset_path, file.contents());
        }

        // Missing files should not be answered with the application.
        if asset_path
            .rsplit('/')
            .next()
            .is_some_and(|name| name.contains('.'))
        {
            return StatusCode::NOT_FOUND.into_response();
        }
    }

    index_response(&state.index, prefix)
}

fn asset_response(path: &str, contents: &'static [u8]) -> Response {
    let content_type = mime_guess::from_path(path).first_or_octet_stream();

    // Files in `assets` are content-hashed by the bundler.
    let cache_control = if path.starts_with("assets/") {
        "public, max-age=31536000, immutable"
    } else {
        "no-cache"
    };

    (
        [
            (
                header::CONTENT_TYPE,
                HeaderValue::from_str(content_type.as_ref())
                    .unwrap_or(HeaderValue::from_static("application/octet-stream")),
            ),
            (
                header::CACHE_CONTROL,
                HeaderValue::from_static(cache_control),
            ),
        ],
        Body::from(contents),
    )
        .into_response()
}

fn index_response(index: &str, prefix: &str) -> Response {
    let base = format!(
        r#"<base href="{}/" />"#,
        escape_attribute(prefix.trim_end_matches('/'))
    );

    (
        [
            (
                header::CONTENT_TYPE,
                HeaderValue::from_static("text/html; charset=utf-8"),
            ),
            (header::CACHE_CONTROL, HeaderValue::from_static("no-cache")),
        ],
        index.replace(BASE_MARKER, &base),
    )
        .into_response()
}

fn escape_attribute(value: &str) -> String {
    let mut escaped = String::with_capacity(value.len());
    for c in value.chars() {
        match c {
            '&' => escaped.push_str("&amp;"),
            '"' => escaped.push_str("&quot;"),
            '\'' => escaped.push_str("&#39;"),
            '<' => escaped.push_str("&lt;"),
            '>' => escaped.push_str("&gt;"),
            c => escaped.push(c),
        }
    }
    escaped
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::http::Request;
    use http_body_util::BodyExt;
    use tower::ServiceExt;

    async fn get(app: Router, path: &str) -> (StatusCode, axum::http::HeaderMap, String) {
        let response = app
            .oneshot(Request::get(path).body(Body::empty()).unwrap())
            .await
            .unwrap();
        let status = response.status();
        let headers = response.headers().clone();
        let body = response.into_body().collect().await.unwrap().to_bytes();
        (status, headers, String::from_utf8_lossy(&body).into_owned())
    }

    fn options() -> UiOptions {
        UiOptions {
            api_url: Some("https://api.example.com/?a=\"b\"".into()),
        }
    }

    #[test]
    fn index_contains_markers() {
        let index = ASSETS
            .get_file("index.html")
            .unwrap()
            .contents_utf8()
            .unwrap();
        assert!(index.contains(BASE_MARKER), "missing base marker");
        assert!(index.contains(API_URL_MARKER), "missing API URL marker");
    }

    #[tokio::test]
    async fn serves_index_at_root() {
        let (status, headers, body) = get(router(options()), "/").await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(headers[header::CACHE_CONTROL], "no-cache");

        assert!(body.contains(r#"<base href="/" />"#));
        assert!(body.contains(
            r#"<meta name="ora-api-url" content="https://api.example.com/?a=&quot;b&quot;" />"#
        ));
    }

    #[tokio::test]
    async fn serves_index_nested() {
        let app = Router::new().nest("/ui", router(UiOptions::default()));

        for path in ["/ui", "/ui/jobs/123"] {
            let (status, _, body) = get(app.clone(), path).await;
            assert_eq!(status, StatusCode::OK, "{path}");
            assert!(body.contains(r#"<base href="/ui/" />"#), "{path}");
        }
    }

    #[tokio::test]
    async fn serves_assets() {
        let app = Router::new().nest("/ui", router(UiOptions::default()));

        let (status, _, _) = get(app.clone(), "/ui/assets/missing.js").await;
        assert_eq!(status, StatusCode::NOT_FOUND);

        let script = ASSETS
            .get_dir("assets")
            .unwrap()
            .files()
            .find(|file| file.path().extension().is_some_and(|ext| ext == "js"))
            .unwrap();

        let (status, headers, _) =
            get(app, &format!("/ui/{}", script.path().to_str().unwrap())).await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(headers[header::CONTENT_TYPE], "text/javascript");
        assert_eq!(
            headers[header::CACHE_CONTROL],
            "public, max-age=31536000, immutable"
        );
    }
}
