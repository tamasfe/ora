# ora-ui

Embedded web UI for inspecting and administrating the [Ora](https://github.com/tamasfe/ora) scheduler.

The UI talks to the Ora admin API via gRPC-web. This crate embeds the built UI
and serves it with [axum](https://docs.rs/axum), typically on the same port as the API:

```rust,ignore
use ora::server::{ServerBuilder, ServerOptions};
use ora_server::proto::admin::v1::admin_service_server::AdminServiceServer;
use tonic_web::GrpcWebLayer;

let server = ServerBuilder::new(backend, ServerOptions::default()).spawn();

// The gRPC-web layer rejects plain HTTP/1 requests, so only apply it to the API routes.
let grpc = tonic::service::Routes::new(AdminServiceServer::new(server.grpc()))
    .into_axum_router()
    .layer(GrpcWebLayer::new());

// Serve the UI at `/ui`.
let app = grpc.nest("/ui", ora_ui::router(ora_ui::UiOptions::default()));

axum::serve(tokio::net::TcpListener::bind("0.0.0.0:50051").await?, app).await?;
```

See `crates/ora/examples/server.rs` for a complete example.

- By default the UI expects the API on the same origin, set `UiOptions::api_url` otherwise
  (the API must then allow the UI's origin via CORS and expose the `grpc-status` and `grpc-message` headers).
- If the API on the other origin requires cookies (e.g. a session behind authentication),
  set `UiOptions::api_credentials`, browsers do not send cookies to other origins otherwise.
  The API's CORS policy must allow credentials, which rules out wildcards:

  ```rust,ignore
  CorsLayer::new()
      .allow_origin(AllowOrigin::mirror_request()) // or the UI's origin
      .allow_credentials(true)
      .allow_methods([Method::POST])
      .allow_headers(AllowHeaders::mirror_request())
      .expose_headers([
          HeaderName::from_static("grpc-status"),
          HeaderName::from_static("grpc-message"),
          HeaderName::from_static("grpc-status-details-bin"),
      ])
  ```

  If the two origins are not on the same site, the session cookie must also be `SameSite=None; Secure`.
- Nested routers do not match the path with a trailing slash (e.g. `/ui/`),
  add a redirect route for it if needed:
  `.route("/ui/", get(|| async { Redirect::permanent("/ui") }))`.
- Responses are not compressed, add `tower_http::compression::CompressionLayer` if needed.
- The published crate contains the built UI. When building from the repository,
  run `cargo xtask ui build` first, the build fails without the assets.
