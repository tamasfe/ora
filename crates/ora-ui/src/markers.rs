// Markers in the UI's `index.html` that are replaced when serving it.
//
// Shared between the build script (which validates the built UI) and the library.

/// Replaced with the path prefix the UI is served under.
const BASE_MARKER: &str = r#"<base href="/" />"#;

/// Replaced with the configured API URL.
const API_URL_MARKER: &str = r#"<meta name="ora-api-url" content="" />"#;
