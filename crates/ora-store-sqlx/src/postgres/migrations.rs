pub use sqlx_migrate::prelude::*;
#[allow(dead_code)]
#[allow(clippy::all, clippy::pedantic)]
/// Created at 20230729110410.
pub mod _1_initial_migration_migrate {}
#[allow(dead_code)]
#[allow(clippy::all, clippy::pedantic)]
/// Created at 20230729110410.
pub mod _1_initial_migration_revert {}
#[allow(dead_code)]
#[allow(clippy::all, clippy::pedantic)]
/// Created at 20230915131205.
pub mod _2_worker_ids_migrate {}
#[allow(dead_code)]
#[allow(clippy::all, clippy::pedantic)]
/// Created at 20230915131205.
pub mod _2_worker_ids_revert {}
#[allow(dead_code)]
#[allow(clippy::all, clippy::pedantic)]
/// Created at 20231129200626.
pub mod _3_worker_registry_migrate {}
#[allow(dead_code)]
#[allow(clippy::all, clippy::pedantic)]
/// Created at 20231129200626.
pub mod _3_worker_registry_revert {}
#[allow(dead_code)]
#[allow(clippy::all, clippy::pedantic)]
/// Created at 20240501200626.
pub mod _4_label_indexes_migrate {}
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
        sqlx_migrate::Migration::new(
                "worker_ids",
                |ctx| std::boxed::Box::pin(async move {
                    use sqlx::Executor;
                    let ctx: &mut sqlx_migrate::prelude::MigrationContext<
                        sqlx::Postgres,
                    > = ctx;
                    ctx.tx()
                        .execute(
                            include_str!(
                                "/home/tamasfe/work/opensauce/ora/master/crates/ora-store-sqlx/migrations/postgres/20230915131205_worker_ids.migrate.sql"
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
                            "/home/tamasfe/work/opensauce/ora/master/crates/ora-store-sqlx/migrations/postgres/20230915131205_worker_ids.revert.sql"
                        ),
                    )
                    .await?;
                Ok(())
            })),
        sqlx_migrate::Migration::new(
                "worker_registry",
                |ctx| std::boxed::Box::pin(async move {
                    use sqlx::Executor;
                    let ctx: &mut sqlx_migrate::prelude::MigrationContext<
                        sqlx::Postgres,
                    > = ctx;
                    ctx.tx()
                        .execute(
                            include_str!(
                                "/home/tamasfe/work/opensauce/ora/master/crates/ora-store-sqlx/migrations/postgres/20231129200626_worker_registry.migrate.sql"
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
                            "/home/tamasfe/work/opensauce/ora/master/crates/ora-store-sqlx/migrations/postgres/20231129200626_worker_registry.revert.sql"
                        ),
                    )
                    .await?;
                Ok(())
            })),
        sqlx_migrate::Migration::new(
            "label_indexes",
            |ctx| std::boxed::Box::pin(async move {
                use sqlx::Executor;
                let ctx: &mut sqlx_migrate::prelude::MigrationContext<sqlx::Postgres> = ctx;
                ctx.tx()
                    .execute(
                        include_str!(
                            "/home/tamasfe/work/opensauce/ora/master/crates/ora-store-sqlx/migrations/postgres/20240501200626_label_indexes.migrate.sql"
                        ),
                    )
                    .await?;
                Ok(())
            }),
        ),
    ]
}
