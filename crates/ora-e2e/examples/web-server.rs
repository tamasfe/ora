//! An example of running the ORA server with gRPC-Web enabled.

use ora_proto::server::v1::admin_service_server::AdminServiceServer;
use ora_server::ServerOptions;
use ora_storage_sqlite::SqliteStorage;
use tonic_web::GrpcWebLayer;
use tower_http::{
    cors::CorsLayer,
    trace::{DefaultOnRequest, DefaultOnResponse},
};
use tracing::Level;

#[tokio::main(flavor = "multi_thread")]
async fn main() -> eyre::Result<()> {
    ora_e2e::util::init_tracing();

    let storage = SqliteStorage::new(rusqlite::Connection::open_in_memory()?)?;

    let server = ora_server::Server::spawn(storage, ServerOptions::default())?;
    ora_e2e::util::log_audit_events(&server);

    let addr = "127.0.0.1:50051".parse()?;

    tonic::transport::Server::builder()
        .accept_http1(true)
        .layer(
            tower_http::trace::TraceLayer::new_for_grpc()
                .on_request(DefaultOnRequest::new().level(Level::INFO))
                .on_response(DefaultOnResponse::new().level(Level::INFO)),
        )
        .layer(CorsLayer::very_permissive())
        .layer(GrpcWebLayer::new())
        .add_service(AdminServiceServer::new(server.admin_service()))
        .add_service(
            ora_proto::server::v1::executor_service_server::ExecutorServiceServer::new(
                server.executor_service(),
            ),
        )
        .serve(addr)
        .await?;

    Ok(())
}
