# Ora Web UI

## Development Setup

- Start a local postgres database, e.g. `podman run --rm -it -e POSTGRES_PASSWORD=postgres -p 5432:5432 postgres`
- `cargo run --example server --features "server executor"`
- `pnpm i` (first time)
- `pnpm dev`

## Proto Code Generation

Run `buf generate ../proto` from this directory.
