# Ora

Ora is a job scheduler written in Rust. It stores jobs and schedules in Postgres, sends executions to executors over gRPC, and handles retries, timeouts, priorities and cron or interval schedules. It also has a client library, a CLI with a TUI and a web UI.

Ora is a set of libraries, not a standalone binary. You embed the server in your own application and expose its gRPC services however you like. Executors run in the same process or connect from somewhere else.

## Features

- **Typed jobs**: a job type is a Rust struct that derives `JobType`. Its JSON schema is registered with the server, so the CLI and the UI can validate input.
- **Scheduling**: run jobs now or at a set time, or repeat them on a fixed interval or a cron expression. You choose whether missed times are skipped or caught up.
- **Retries and timeouts**: set the retry count, use fixed or exponential backoff with an optional cap, and measure timeouts from the target time or the start time.
- **Priorities**: when executor capacity runs out, higher-priority jobs run first. Jobs with the same priority run in the order they were created.
- **Executors**: async handlers with a concurrency limit per handler, cooperative cancellation, attempt numbers, and admission guards for pausing or rejecting work.
- **Labels and filtering**: attach key/value labels to jobs and schedules, then filter, count, cancel or deduplicate by them (`add_job_if_not_exists`).
- **History retention**: optionally delete inactive history after a set time.
- **Tooling**: a gRPC admin API, the `ora` CLI and TUI, a Vue web UI served from the `ora-ui` crate, and a benchmark suite.

## Crates

| Crate | Description |
| --- | --- |
| [`ora`](crates/ora) | Client library: job types, the admin client, executors. The `server` feature re-exports the server and `executor` enables executors. |
| [`ora-server`](crates/ora-server) | The scheduler server and its gRPC services, built on any `Backend`. |
| [`ora-backend`](crates/ora-backend) | The storage and timing `Backend` trait, plus a conformance test suite. |
| [`ora-backend-postgres`](crates/ora-backend-postgres) | The Postgres backend, which runs its migrations into the `ora` schema. |
| [`ora-macros`](crates/ora-macros) | `#[derive(JobType)]`. |
| [`ora-ui`](crates/ora-ui) | The web UI, built and embedded in the crate and served with axum. |
| [`ora-cli`](crates/ora-cli) | The `ora` command-line client and TUI. |
| [`ora-bench`](crates/ora-bench) | A benchmark suite that runs against any Ora server over gRPC. |

The protobuf definitions are in [`proto/`](proto) and the web UI source is in [`ui/`](ui).

## Quick start

### Define a job type

```rust
use ora::JobType;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

/// Return the character count in the given string.
#[derive(Debug, JobType, Serialize, Deserialize, JsonSchema)]
#[ora(namespace = "text", output = "u64")]
struct CountChars {
    value: String,
}
```

The job type ID comes from either `namespace` (namespace plus type name) or an explicit `name`. You must give exactly one of them. `output` defaults to `()`.

### Run a server and an executor

```rust
use ora::{
    AdminClient, IntoJob,
    executor::{Executor, HandlerOptions},
    server::{ServerBuilder, ServerHandleExt, ServerOptions},
};
use ora_backend_postgres::PostgresBackend;

let backend = PostgresBackend::new(pool).await?; // a deadpool_postgres::Pool
let server = ServerBuilder::new(backend, ServerOptions::default()).spawn();

let _executor = Executor::new(server.execution_client())
    .with_name("in_process")
    .handler(async |_ctx, job: CountChars| Ok(job.value.chars().count() as u64))
    .spawn();
```

`server.admin_client()` and `server.execution_client()` connect over an in-memory transport. To serve over the network, mount `AdminServiceServer::new(server.grpc())` and `ExecutionServiceServer::new(server.grpc())` on a tonic or axum router. Executors in other processes connect with a normal `ExecutionServiceClient`.

### Add jobs and schedules

```rust
use std::time::Duration;
use ora::job::{BackoffStrategy, TimeoutBaseTime};

let admin = AdminClient::new(server.admin_client());

let mut job = admin
    .add_job(
        CountChars { value: "hello".into() }
            .now()
            .with_label("source", "readme")
            .with_retries(3)
            .with_retry_backoff(Duration::from_secs(1))
            .with_retry_backoff_strategy(BackoffStrategy::Exponential)
            .with_timeout(Duration::from_secs(30), TimeoutBaseTime::StartTime)
            .with_priority(10),
    )
    .await?;

let count: u64 = job.wait_result().await?;

// Every hour, starting now.
admin
    .add_schedule(CountChars { value: "tick".into() }.schedule_interval(Duration::from_secs(3600)).immediate())
    .await?;

// Every day at midnight.
admin
    .add_schedule(CountChars { value: "daily".into() }.now().schedule_cron("0 0 * * *")?)
    .await?;
```

[`crates/ora/examples/server.rs`](crates/ora/examples/server.rs) is a complete example with several job types, an admission guard, gRPC-web, CORS and the embedded UI.

## Running the example

Start Postgres, build the UI assets, then run the example server:

```sh
podman run --rm -it -e POSTGRES_PASSWORD=postgres -p 5432:5432 postgres
cargo xtask ui build
cargo run -p ora --example server --features "server executor"
```

The API and the web UI are both served at <http://localhost:50051> (the UI is at `/ui`). You can configure them with these environment variables:

- `ORA_ADDR`: the listen address, `0.0.0.0:50051` by default.
- `ORA_DATABASE_URL`: the Postgres URL, `postgresql://postgres:postgres@localhost:5432/postgres` by default.

## CLI

```sh
cargo install --path crates/ora-cli
export ORA_URL=http://localhost:50051

ora                  # start the TUI
ora types list
ora jobs add --type text.CountChars '{"value": "hello"}' --label source=cli
ora jobs list
ora schedules list
ora executors list
ora maintenance --help
```

Use `--help` on any command for its full options. When the job payload is `-`, the CLI reads it from stdin. If `EDITOR` is set, the CLI opens the payload in that editor before submitting.

## Web UI

The UI is a Vue app in [`ui/`](ui). It talks to the admin API over gRPC-web. The `ora-ui` crate embeds the built assets and gives you an axum router to mount next to the API. See [`crates/ora-ui/README.md`](crates/ora-ui/README.md) for the embedding, CORS and cookie notes, and [`ui/README.md`](ui/README.md) for the frontend dev setup.

## Benchmarks

```sh
cargo run -p ora-bench --release -- --url http://localhost:50051           # all scenarios
cargo run -p ora-bench --release -- --url http://localhost:50051 latency   # one scenario
```

The scenarios are `rpc`, `latency`, `delayed`, `throughput`, `retry` and `cancel`. The target server must expose both the admin and the execution services. Use `--json <file>` to save the full report.

## Development

```sh
cargo xtask codegen proto   # regenerate the Rust protobuf code
cargo xtask ui build        # build the web UI into crates/ora-ui (needed to build ora-ui)
cargo test --workspace --all-features
```

The end-to-end and Postgres backend tests need a running database. Set it with `ORA_TEST_DATABASE_URL` (default `postgresql://postgres:postgres@localhost:5432/postgres`). **The tests drop the `ora` schema in that database.** [`docker/postgres.Dockerfile`](docker/postgres.Dockerfile) builds a Postgres 18 image with the `hypopg` extension installed, which helps with query tuning.
