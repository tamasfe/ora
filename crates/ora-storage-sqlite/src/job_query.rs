use core::fmt::Write;

use ora_storage::{JobQueryFilters, JobQueryOrder, JobQueryResult};
use rusqlite::Transaction;
use sea_query::{Expr, Query, SelectStatement, SqliteQueryBuilder};
use sea_query_rusqlite::RusqliteBinder;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::models::{JobRetryPolicy, JobTimeoutPolicy, SqlExecutionStatus, SqlSystemTime};

pub(super) fn count_jobs(tx: &mut Transaction, filters: JobQueryFilters) -> eyre::Result<u64> {
    let mut select_query = Query::select();
    select_query.expr(Expr::cust("COUNT(*)")).from(I("ora_job"));

    filter_jobs_query(&mut select_query, filters, None);

    let (query, values) = select_query.build_rusqlite(SqliteQueryBuilder);

    let mut statement = tx.prepare(&query)?;
    let mut rows = statement.query(&*values.as_params())?;

    let count: i64 = rows.next()?.unwrap().get(0)?;

    Ok(u64::try_from(count).unwrap())
}

pub(super) fn job_ids(tx: &mut Transaction, filters: JobQueryFilters) -> eyre::Result<Vec<Uuid>> {
    let mut select_query = Query::select();
    select_query.column(I("id")).from(I("ora_job"));
    filter_jobs_query(&mut select_query, filters, None);
    select_query.order_by(I("id"), sea_query::Order::Asc);

    let (query, values) = select_query.build_rusqlite(SqliteQueryBuilder);

    let mut statement = tx.prepare(&query)?;
    let mut rows = statement.query(&*values.as_params())?;

    let mut job_ids = Vec::new();
    while let Some(row) = rows.next()? {
        job_ids.push(row.get(0)?);
    }

    Ok(job_ids)
}

pub(super) fn delete_jobs(
    tx: &mut Transaction,
    filters: JobQueryFilters,
) -> eyre::Result<Vec<Uuid>> {
    tx.execute(
        "CREATE TEMP TABLE IF NOT EXISTS temp.query_jobs(id BLOB)",
        [],
    )?;

    {
        let mut select_query = Query::select();
        select_query.column(I("id")).from(I("ora_job"));

        filter_jobs_query(&mut select_query, filters, None);

        let (query, values) = Query::insert()
            .into_table((I("temp"), I("query_jobs")))
            .columns([I("id")])
            .select_from(select_query)?
            .build_rusqlite(SqliteQueryBuilder);

        let mut statement = tx.prepare(&query)?;
        statement.execute(&*values.as_params())?;
    }

    let deleted_jobs: Vec<Uuid> = tx
        .prepare(
            r#"--sql
        DELETE FROM
            "ora_job"
        WHERE
            "id" IN (
                SELECT
                    "id"
                FROM
                    temp.query_jobs
            )
        RETURNING
            "id"
        "#,
        )?
        .query_map([], |row| row.get(0))?
        .collect::<Result<Vec<_>, _>>()?;

    tx.execute(
        r#"--sql
        DELETE FROM
            "ora_job_label"
        WHERE
            "job_id" IN (
                SELECT
                    "id"
                FROM
                    temp.query_jobs
            )
        "#,
        [],
    )?;

    tx.execute(
        r#"--sql
        DELETE FROM
            "ora_execution"
        WHERE
            "job_id" IN (
                SELECT
                    "id"
                FROM
                    temp.query_jobs
            )
        "#,
        [],
    )?;

    tx.execute("DROP TABLE temp.query_jobs", [])?;

    Ok(deleted_jobs)
}

pub(super) fn query_job_details(
    tx: &mut Transaction,
    cursor: Option<Cursor>,
    limit: usize,
    order: JobQueryOrder,
    filters: JobQueryFilters,
) -> eyre::Result<JobQueryResult> {
    let (last_job_id, filters, order) = match cursor {
        Some(cursor) => (cursor.last_job_id, cursor.filters, cursor.order),
        None => (None, filters, order),
    };

    tx.execute(
        "CREATE TEMP TABLE IF NOT EXISTS temp.query_jobs(id BLOB)",
        [],
    )?;

    {
        let mut select_query = Query::select();
        select_query.column(I("id")).from(I("ora_job"));

        filter_jobs_query(&mut select_query, filters.clone(), last_job_id);
        order_jobs_query(&mut select_query, order);

        let (query, values) = Query::insert()
            .into_table((I("temp"), I("query_jobs")))
            .columns([I("id")])
            .select_from(select_query)?
            .build_rusqlite(SqliteQueryBuilder);

        let mut statement = tx.prepare(&query)?;
        statement.execute(&*values.as_params())?;
    }

    let total_count: usize = tx.query_row(
        r#"--sql
        SELECT
            COUNT(*)
        FROM
            temp.query_jobs
        "#,
        [],
        |row| row.get(0),
    )?;

    let mut jobs = Vec::new();

    {
        let mut stmt = tx.prepare_cached(
            r#"--sql
            SELECT
                "ora_job"."id",
                (
                    "marked_unschedulable_at_unix_ns" IS NULL
                    OR EXISTS (
                        SELECT
                            1
                        FROM
                            "ora_execution"
                        WHERE
                            "status" IN (0, 1, 2)
                            AND "job_id" = "ora_job"."id"
                    )
                ) AS "active",
                cancelled_at_unix_ns IS NOT NULL AS "cancelled",
                "job_type_id",
                "schedule_id",
                "target_execution_time_unix_ns",
                "input_payload_json",
                "metadata_json",
                "created_at_unix_ns",
                "timeout_policy",
                "retry_policy"
            FROM
                "ora_job"
            JOIN
                temp.query_jobs
            ON
                "ora_job"."id" = temp.query_jobs."id"
            LIMIT ?
            "#,
        )?;

        let mut rows = stmt.query([limit])?;

        while let Some(row) = rows.next()? {
            let id: Uuid = row.get(0)?;
            let active: bool = row.get(1)?;
            let cancelled: bool = row.get(2)?;
            let job_type_id: String = row.get(3)?;
            let schedule_id: Option<Uuid> = row.get(4)?;
            let target_execution_time: SqlSystemTime = row.get(5)?;
            let input_payload_json: String = row.get(6)?;
            let metadata_json: Option<String> = row.get(7)?;
            let created_at: SqlSystemTime = row.get(8)?;
            let timeout_policy: JobTimeoutPolicy = row.get(9)?;
            let retry_policy: JobRetryPolicy = row.get(10)?;

            jobs.push(ora_storage::JobDetails {
                active,
                cancelled,
                id,
                job_type_id,
                schedule_id,
                target_execution_time: target_execution_time.into(),
                input_payload_json,
                timeout_policy: timeout_policy.into(),
                retry_policy: retry_policy.into(),
                created_at: created_at.into(),
                metadata_json,
                labels: Default::default(),
                executions: Default::default(),
            });
        }
    }

    {
        let mut stmt = tx.prepare_cached(
            r#"--sql
            SELECT
                "job_id",
                "ora_execution"."id",
                "executor_id",
                "created_at_unix_ns",
                "ready_at_unix_ns",
                "assigned_at_unix_ns",
                "started_at_unix_ns",
                "succeeded_at_unix_ns",
                "failed_at_unix_ns",
                "output_payload_json",
                "failure_reason",
                "status"
            FROM
                "ora_execution"
            JOIN
                temp.query_jobs
            ON
                "ora_execution"."job_id" = temp.query_jobs."id"
            ORDER BY
                "ora_execution"."id" ASC
            "#,
        )?;

        let mut rows = stmt.query([])?;

        while let Some(row) = rows.next()? {
            let job_id: Uuid = row.get(0)?;
            let id: Uuid = row.get(1)?;
            let executor_id: Option<Uuid> = row.get(2)?;
            let created_at: SqlSystemTime = row.get(3)?;
            let ready_at: Option<SqlSystemTime> = row.get(4)?;
            let assigned_at: Option<SqlSystemTime> = row.get(5)?;
            let started_at: Option<SqlSystemTime> = row.get(6)?;
            let succeeded_at: Option<SqlSystemTime> = row.get(7)?;
            let failed_at: Option<SqlSystemTime> = row.get(8)?;
            let output_payload_json: Option<String> = row.get(9)?;
            let failure_reason: Option<String> = row.get(10)?;
            let status: SqlExecutionStatus = row.get(11)?;

            let execution = ora_storage::ExecutionDetails {
                id,
                job_id,
                executor_id,
                status: status.into(),
                created_at: created_at.into(),
                ready_at: ready_at.map(Into::into),
                assigned_at: assigned_at.map(Into::into),
                started_at: started_at.map(Into::into),
                succeeded_at: succeeded_at.map(Into::into),
                failed_at: failed_at.map(Into::into),
                output_payload_json,
                failure_reason,
            };

            if let Some(job) = jobs.iter_mut().find(|job| job.id == job_id) {
                job.executions.push(execution);
            }
        }
    }

    {
        let mut stmt = tx.prepare_cached(
            r#"--sql
            SELECT
                "job_id",
                "key",
                "value"
            FROM
                "ora_job_label"
            JOIN
                temp.query_jobs
            ON
                "ora_job_label"."job_id" = temp.query_jobs."id"
            "#,
        )?;

        let mut rows = stmt.query([])?;

        while let Some(row) = rows.next()? {
            let job_id: Uuid = row.get(0)?;
            let key: String = row.get(1)?;
            let value: String = row.get(2)?;

            if let Some(job) = jobs.iter_mut().find(|job| job.id == job_id) {
                job.labels.insert(key, value);
            }
        }
    }

    tx.execute("DROP TABLE temp.query_jobs", [])?;

    let new_cursor = Cursor {
        last_job_id: jobs.last().map(|job| job.id),
        filters,
        order,
    };

    Ok(JobQueryResult {
        has_more: jobs.len() < total_count,
        cursor: Some(serde_json::to_string(&new_cursor).unwrap()),
        jobs,
    })
}

fn filter_jobs_query(query: &mut SelectStatement, filters: JobQueryFilters, after: Option<Uuid>) {
    let JobQueryFilters {
        job_ids,
        job_type_ids,
        execution_ids,
        schedule_ids,
        execution_status,
        labels,
        active,
    } = filters;

    if let Some(after) = after {
        query.and_where(Expr::col(I("id")).gt(sea_query::Value::Uuid(Some(Box::new(after)))));
    }

    if let Some(job_ids) = job_ids {
        query.and_where(
            Expr::col(I("id")).is_in(
                job_ids
                    .into_iter()
                    .map(|id| sea_query::Value::Uuid(Some(Box::new(id)))),
            ),
        );
    }

    if let Some(job_type_ids) = job_type_ids {
        query.and_where(
            Expr::col(I("job_type_id")).is_in(
                job_type_ids
                    .into_iter()
                    .map(|id| sea_query::Value::String(Some(Box::new(id)))),
            ),
        );
    }

    if let Some(execution_ids) = execution_ids {
        let mut subquery = Query::select();
        subquery
            .expr(Expr::value(1))
            .from(I("ora_execution"))
            .and_where(Expr::col(I("job_id")).equals((I("ora_job"), I("id"))))
            .and_where(
                Expr::col((I("ora_execution"), I("id"))).is_in(
                    execution_ids
                        .into_iter()
                        .map(|id| sea_query::Value::Uuid(Some(Box::new(id)))),
                ),
            );

        query.and_where(Expr::exists(subquery));
    }

    if let Some(schedule_ids) = schedule_ids {
        let mut subquery = Query::select();
        subquery
            .expr(Expr::value(1))
            .from(I("ora_schedule"))
            .and_where(
                Expr::col((I("ora_schedule"), I("id"))).equals((I("ora_job"), I("schedule_id"))),
            )
            .and_where(
                Expr::col((I("ora_schedule"), I("id"))).is_in(
                    schedule_ids
                        .into_iter()
                        .map(|id| sea_query::Value::Uuid(Some(Box::new(id)))),
                ),
            );

        query.and_where(Expr::exists(subquery));
    }

    if let Some(execution_status) = execution_status {
        let mut last_execution_status = Query::select();
        last_execution_status
            .column(I("status"))
            .from(I("ora_execution"))
            .and_where(Expr::col(I("job_id")).equals((I("ora_job"), I("id"))))
            .order_by((I("ora_execution"), I("id")), sea_query::Order::Desc)
            .limit(1);

        for status in execution_status {
            let status = SqlExecutionStatus::from(status);

            if status == SqlExecutionStatus::Pending {
                query.and_where(Expr::cust_with_values(
                    r#"--sql
                        (
                            NOT EXISTS
                            (
                                SELECT
                                    1
                                FROM
                                    "ora_execution"
                                WHERE
                                    "job_id" = "ora_job"."id"
                                ORDER BY
                                    "id" DESC
                                LIMIT 1
                            ) OR (
                                SELECT
                                    "status"
                                FROM
                                    "ora_execution"
                                WHERE
                                    "job_id" = "ora_job"."id"
                                ORDER BY
                                    "id" DESC
                                LIMIT 1
                            ) = ?
                        )
                        "#,
                    [sea_query::Value::BigInt(Some(status as _))],
                ));
            } else {
                query.and_where(Expr::cust_with_values(
                    r#"--sql
                        (
                            SELECT
                                "status"
                            FROM
                                "ora_execution"
                            WHERE
                                "job_id" = "ora_job"."id"
                            ORDER BY
                                "id" DESC
                            LIMIT 1
                        ) = ?
                        "#,
                    [sea_query::Value::BigInt(Some(status as _))],
                ));
            }
        }
    }

    if let Some(labels) = labels {
        for (key, value) in labels {
            match value {
                ora_storage::JobLabelFilterValue::Exists => {
                    query.and_where(Expr::cust_with_values(
                        r#"--sql
                                EXISTS
                                (
                                    SELECT
                                        1
                                    FROM
                                        "ora_job_label"
                                    WHERE
                                        "job_id" = "ora_job"."id"
                                        AND "key" = ?
                                )
                                "#,
                        [sea_query::Value::String(Some(Box::new(key)))],
                    ));
                }
                ora_storage::JobLabelFilterValue::Equals(value) => {
                    query.and_where(Expr::cust_with_values(
                        r#"--sql
                                EXISTS
                                (
                                    SELECT
                                        1
                                    FROM
                                        "ora_job_label"
                                    WHERE
                                        "job_id" = "ora_job"."id"
                                        AND "key" = ?
                                        AND "value" = ?
                                )
                                "#,
                        [
                            sea_query::Value::String(Some(Box::new(key))),
                            sea_query::Value::String(Some(Box::new(value))),
                        ],
                    ));
                }
            }
        }
    }

    if let Some(active) = active {
        let mut active_expr = Expr::cust(
            r#"--sql
                (
                    "marked_unschedulable_at_unix_ns" IS NULL
                    OR EXISTS (
                        SELECT
                            1
                        FROM
                            "ora_execution"
                        WHERE
                            "status" IN (0, 1, 2)
                            AND "job_id" = "ora_job"."id"
                    )
                )
                "#,
        );

        if !active {
            active_expr = active_expr.not();
        }

        query.and_where(active_expr);
    }
}

fn order_jobs_query(query: &mut SelectStatement, order: JobQueryOrder) {
    match order {
        JobQueryOrder::CreatedAtAsc => {
            query.order_by((I("ora_job"), I("id")), sea_query::Order::Asc);
        }
        JobQueryOrder::CreatedAtDesc => {
            query.order_by((I("ora_job"), I("id")), sea_query::Order::Desc);
        }
        JobQueryOrder::TargetExecutionTimeAsc => {
            query.order_by(
                (I("ora_job"), I("target_execution_time_unix_ns")),
                sea_query::Order::Asc,
            );
        }
        JobQueryOrder::TargetExecutionTimeDesc => {
            query.order_by(
                (I("ora_job"), I("target_execution_time_unix_ns")),
                sea_query::Order::Desc,
            );
        }
    }
}

#[derive(Debug, Serialize, Deserialize)]
pub(super) struct Cursor {
    pub(super) last_job_id: Option<Uuid>,
    pub(super) filters: JobQueryFilters,
    pub(super) order: JobQueryOrder,
}

struct I(&'static str);

impl sea_query::Iden for I {
    fn unquoted(&self, s: &mut dyn Write) {
        s.write_str(self.0).unwrap();
    }
}
