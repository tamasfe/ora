//! Utilities and wrappers for working with the database.

use std::borrow::Cow;

use tokio_postgres::Row;
use tracing::Instrument;

macro_rules! dump_sql {
    ($q:expr) => {
        #[cfg(feature = "__dump_sql")]
        {
            fn hash_query(sql: &str) -> u64 {
                let mut hash: u64 = 0xcbf29ce484222325;
                for b in sql.as_bytes() {
                    hash ^= *b as u64;
                    hash = hash.wrapping_mul(0x100000001b3);
                }
                hash
            }

            std::fs::create_dir_all(".local.queries").unwrap();
            std::fs::write(format!(".local.queries/{}.sql", hash_query($q)), $q).unwrap();
        }
    };
}

/// A simple wrapper over a database pool for tracing purposes.
pub(crate) struct DbPool(pub(crate) deadpool_postgres::Pool);

impl DbPool {
    pub(crate) async fn get(&self) -> Result<DbConnection, deadpool_postgres::PoolError> {
        

        Ok(DbConnection {
            conn: self.0.get().await?,
            span: tracing::info_span!("connection"),
        })
    }
}
pub(crate) struct DbConnection {
    conn: deadpool_postgres::Object,
    span: tracing::Span,
}

impl DbConnection {
    pub(crate) async fn transaction(&mut self) -> Result<DbTransaction<'_>, tokio_postgres::Error> {
        Ok(DbTransaction {
            tx: self
                .conn
                .transaction()
                .instrument(self.span.clone())
                .await?,
            span: tracing::info_span!(parent: &self.span, "transaction"),
        })
    }

    pub(crate) async fn read_only_transaction(
        &mut self,
    ) -> Result<DbTransaction<'_>, tokio_postgres::Error> {
        Ok(DbTransaction {
            tx: self
                .conn
                .build_transaction()
                .read_only(true)
                .start()
                .instrument(self.span.clone())
                .await?,
            span: tracing::info_span!(parent: &self.span, "transaction"),
        })
    }
}

pub(crate) struct DbTransaction<'a> {
    tx: deadpool_postgres::Transaction<'a>,
    span: tracing::Span,
}

impl DbTransaction<'_> {
    pub(crate) async fn commit(self) -> Result<(), tokio_postgres::Error> {
        self.tx.commit().instrument(self.span).await
    }

    pub(crate) async fn prepare(
        &self,
        query: &'static str,
    ) -> Result<DbPreparedStatement, tokio_postgres::Error> {
        dump_sql!(query);

        let stmt = self
            .tx
            .prepare_cached(query)
            .instrument(self.span.clone())
            .await?;

        Ok(DbPreparedStatement {
            inner: stmt,
            query: Cow::Borrowed(query),
        })
    }

    pub(crate) async fn prepare_owned(
        &self,
        query: impl Into<Cow<'static, str>>,
    ) -> Result<DbPreparedStatement, tokio_postgres::Error> {
        let query = query.into();

        dump_sql!(&*query);

        let stmt = self
            .tx
            .prepare_cached(&query)
            .instrument(self.span.clone())
            .await?;

        Ok(DbPreparedStatement { inner: stmt, query })
    }

    pub(crate) async fn execute(
        &self,
        stmt: &DbPreparedStatement,
        params: &[&(dyn tokio_postgres::types::ToSql + Sync)],
    ) -> Result<u64, tokio_postgres::Error> {
        self.tx
            .execute(&stmt.inner, params)
            .instrument(stmt.query_span(&self.span))
            .await
    }

    pub(crate) async fn query(
        &self,
        stmt: &DbPreparedStatement,
        params: &[&(dyn tokio_postgres::types::ToSql + Sync)],
    ) -> Result<Vec<Row>, tokio_postgres::Error> {
        self.tx
            .query(&stmt.inner, params)
            .instrument(stmt.query_span(&self.span))
            .await
    }

    pub(crate) async fn query_one(
        &self,
        stmt: &DbPreparedStatement,
        params: &[&(dyn tokio_postgres::types::ToSql + Sync)],
    ) -> Result<Row, tokio_postgres::Error> {
        self.tx
            .query_one(&stmt.inner, params)
            .instrument(stmt.query_span(&self.span))
            .await
    }

    pub(crate) async fn query_opt(
        &self,
        stmt: &DbPreparedStatement,
        params: &[&(dyn tokio_postgres::types::ToSql + Sync)],
    ) -> Result<Option<Row>, tokio_postgres::Error> {
        self.tx
            .query_opt(&stmt.inner, params)
            .instrument(stmt.query_span(&self.span))
            .await
    }
}

pub(crate) struct DbPreparedStatement {
    inner: tokio_postgres::Statement,
    query: Cow<'static, str>,
}

impl DbPreparedStatement {
    fn query_span(&self, parent: &tracing::Span) -> tracing::Span {
        tracing::info_span!(parent: parent, "query", query = self.query.as_ref())
    }
}
