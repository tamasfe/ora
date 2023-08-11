pub use sqlx_migrate::prelude::*;
#[allow(dead_code)]
#[allow(clippy::all, clippy::pedantic)]
/// Created at 20230729110410.
pub mod _1_initial_migration_migrate {}
#[allow(dead_code)]
#[allow(clippy::all, clippy::pedantic)]
/// Created at 20230729110410.
pub mod _1_initial_migration_revert {}
/// All the migrations.
pub fn migrations() -> impl IntoIterator<Item = Migration<sqlx::Postgres>> {
    [
        sqlx_migrate::Migration::new(
                "initial_migration",
                |ctx| std::boxed::Box::pin(async move {
                    use sqlx::Executor;
                    let ctx: &mut sqlx_migrate::prelude::MigrationContext<
                        sqlx::Postgres,
                    > = ctx;
                    ctx.tx()
                        .execute(
                            include_str!(
                                "/home/tamasfe/work/opensauce/ora/master/crates/ora-store-sqlx/migrations/postgres/20230729110410_initial_migration.migrate.sql"
                            ),
                        )
                        .await?;
                    Ok(())
                }),
            )
            .reversible(|ctx| std::boxed::Box::pin(async move {
                use sqlx::Executor;
                let ctx: &mut sqlx_migrate::prelude::MigrationContext<sqlx::Postgres> = ctx;
                ctx.tx()
                    .execute(
                        include_str!(
                            "/home/tamasfe/work/opensauce/ora/master/crates/ora-store-sqlx/migrations/postgres/20230729110410_initial_migration.revert.sql"
                        ),
                    )
                    .await?;
                Ok(())
            })),
    ]
}
