# Ora Web UI

## Development Setup

- Start a local postgres database, e.g. `podman run --rm -it -e POSTGRES_PASSWORD=postgres -p 5432:5432 postgres`
- `cargo run --example server --features "server executor"`
- `pnpm i` (first time)
- `pnpm dev`

The development server talks to the API configured in `.env.development` (`VITE_ORA_API_URL`).

## Proto Code Generation

Run `buf generate ../proto` from this directory.

## Embedding

The UI is distributed as the `ora-ui` crate, which embeds the built assets and serves them with axum.

Build the assets into the crate with `cargo xtask ui build` from the repository root,
building `ora-ui` fails without them.
To try it, run `cargo run --example server --features "server executor"`
and open <http://127.0.0.1:50051/ui>.

When embedded, the server sets the path prefix (`<base href>`) and optionally
the API URL (`<meta name="ora-api-url">`) in `index.html`,
by default the API is expected on the same origin as the UI.
