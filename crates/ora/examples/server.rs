//! A simple executable that starts the ora server with several executors and example job types.

use deadpool_postgres::{Config, ManagerConfig, RecyclingMethod, Runtime, tokio_postgres::NoTls};
use http::Method;
use ora::{
    JobType,
    executor::{Executor, HandlerOptions},
    server::ServerHandleExt,
};
use ora_backend_postgres::PostgresBackend;
use ora_server::{
    ServerBuilder, ServerOptions, proto::admin::v1::admin_service_server::AdminServiceServer,
};
use ora_ui::UiOptions;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use tonic_web::GrpcWebLayer;
use tower_http::{
    LatencyUnit,
    cors::{Any, CorsLayer},
    trace::{DefaultOnRequest, DefaultOnResponse, TraceLayer},
};
use tracing::Level;
use tracing_subscriber::{Registry, layer::SubscriberExt, util::SubscriberInitExt};

/// Return the character count in the given string.
#[derive(Debug, JobType, Serialize, Deserialize, JsonSchema)]
#[ora(namespace = "text", output = "u64")]
struct CountChars {
    value: String,
}

/// Succeed after the n-th attempt only.
#[derive(Debug, JobType, Serialize, Deserialize, JsonSchema)]
#[ora(namespace = "flaky")]
struct SucceedAfter {
    succeed_on_attempt: u64,
}

/// Always fail.
#[derive(Debug, JobType, Serialize, Deserialize, JsonSchema)]
#[ora(namespace = "")]
struct AlwaysFail {}

/// Run until cancelled.
#[derive(Debug, JobType, Serialize, Deserialize, JsonSchema)]
#[ora(namespace = "cancellable")]
struct RunUntilCancelled {}

/// Run until the given time.
#[derive(Debug, JobType, Serialize, Deserialize, JsonSchema)]
#[ora(namespace = "time")]
struct RunUntil {
    seconds: u64,
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    Registry::default()
        .with(tracing_subscriber::fmt::layer())
        .with(tracing_subscriber::filter::EnvFilter::from_default_env())
        .init();

    let server = ServerBuilder::new(create_backend().await, ServerOptions::default()).spawn();

    let _executor = Executor::new(server.execution_client())
        .with_name("in_process")
        .handler(async |_ctx, job: CountChars| Ok(job.value.chars().count().try_into().unwrap()))
        .handler(async |_ctx, job: RunUntil| {
            let now = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_secs();
            if now < job.seconds {
                tokio::time::sleep(std::time::Duration::from_secs(job.seconds - now)).await;
            }
            Ok(())
        })
        .handler(async |ctx, _: RunUntilCancelled| {
            ctx.cancelled().await;
            Ok(())
        })
        .handler_with_options(
            async |_, _: AlwaysFail| {
                tokio::time::sleep(std::time::Duration::from_secs(5)).await;

                eyre::bail!("job is supposed to fail")
            },
            HandlerOptions { max_concurrent: 10 },
        )
        .handler(async |ctx, job: SucceedAfter| {
            let attempt = ctx.attempt_number();
            if attempt < job.succeed_on_attempt {
                eyre::bail!("failing attempt {attempt}");
            }
            Ok(())
        })
        .spawn();

    let addr: std::net::SocketAddr = std::env::var("ORA_ADDR")
        .unwrap_or_else(|_| "0.0.0.0:50051".into())
        .parse()?;
    let admin_service = AdminServiceServer::new(server.grpc());

    let app = tonic::service::Routes::new(admin_service)
        .into_axum_router()
        .layer(GrpcWebLayer::new())
        .layer(
            TraceLayer::new_for_grpc()
                .on_request(DefaultOnRequest::new().level(Level::INFO))
                .on_response(
                    DefaultOnResponse::new()
                        .level(Level::INFO)
                        .latency_unit(LatencyUnit::Micros),
                ),
        )
        .route(
            "/",
            axum::routing::get(|| async { axum::response::Redirect::temporary("/ui") }),
        )
        .route(
            "/ui/",
            axum::routing::get(|| async { axum::response::Redirect::temporary("/ui") }),
        )
        .nest("/ui", ora_ui::router(UiOptions::default()))
        .layer(
            CorsLayer::new()
                .allow_headers(Any)
                .allow_methods([Method::GET, Method::POST])
                .allow_origin(Any)
                .expose_headers(Any),
        );

    tracing::info!(%addr, "serving the API and the web UI at /ui");
    axum::serve(tokio::net::TcpListener::bind(addr).await?, app).await?;

    Ok(())
}

async fn create_backend() -> PostgresBackend {
    let mut cfg = Config::new();
    cfg.url = Some("postgresql://postgres:postgres@localhost:5432/postgres".to_string());
    cfg.manager = Some(ManagerConfig {
        recycling_method: RecyclingMethod::Fast,
    });
    let pool = cfg.create_pool(Some(Runtime::Tokio1), NoTls).unwrap();

    ora_backend_postgres::PostgresBackend::new(pool)
        .await
        .unwrap()
}
