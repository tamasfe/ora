use std::path::Path;

fn main() {
    sqlx_migrate::generate(
        Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("migrations")
            .join("postgres"),
        Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("src")
            .join("postgres")
            .join("migrations.rs"),
        sqlx_migrate::DatabaseType::Postgres,
    );
}
