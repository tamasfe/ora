use std::{str::FromStr, time::SystemTime};

use base64::Engine;
use ora_backend::{
    common::{Label, NextPageToken},
    executions::{ExecutionDetails, ExecutionId, ExecutionStatus},
    executors::ExecutorId,
    jobs::{CancelledJob, JobDefinition, JobDetails, JobFilters, JobId, JobOrderBy, JobTypeId},
    schedules::ScheduleId,
};
use sea_query::{
    Asterisk, Expr, ExprTrait, JoinType, Order, PostgresQueryBuilder, Query, SelectStatement, Value,
};
use sea_query_postgres::PostgresBinder;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::{
    db::DbTransaction,
    models::PgExecutionStatus,
    util::{systemtime_from_ts, systemtime_to_ts},
};

pub(crate) async fn cancel_jobs(
    tx: &DbTransaction<'_>,
    mut filters: JobFilters,
) -> crate::Result<Vec<CancelledJob>> {
    // We don't cancel jobs that are already in a finished state.
    filters.execution_statuses = match filters.execution_statuses {
        Some(mut s) => {
            // We allow cancelling just the pending or in progress jobs,
            // if they are specified.
            s.retain(|st| matches!(st, ExecutionStatus::Pending | ExecutionStatus::InProgress));

            if s.is_empty() {
                s.push(ExecutionStatus::Pending);
                s.push(ExecutionStatus::InProgress);
            }

            Some(s)
        }
        None => Some(vec![ExecutionStatus::InProgress, ExecutionStatus::Pending]),
    };

    let (query, values) = Query::update()
        .table(("ora", "execution"))
        .value("cancelled_at", Expr::cust("NOW()"))
        .and_where(Expr::col(("ora", "execution", "job_id")).in_subquery(select_job_ids(&filters)))
        // Only the active execution is cancelled, earlier (failed) executions are kept as-is,
        // so that each job is returned once.
        .and_where(Expr::cust("ora.execution.status < 2"))
        .returning(sea_query::ReturningClause::Columns(vec![
            ("ora", "execution", "job_id").into(),
            ("ora", "execution", "id").into(),
        ]))
        .build_postgres(PostgresQueryBuilder);

    let stmt = tx.prepare_owned(query).await?;

    let rows = tx.query(&stmt, &values.as_params()).await?;

    let mut cancelled_jobs = Vec::with_capacity(rows.len());

    for row in rows {
        let job_id = JobId(row.try_get(0)?);
        let last_execution_id = ExecutionId(row.try_get(1)?);

        cancelled_jobs.push(CancelledJob {
            job_id,
            last_execution_id,
        });
    }

    Ok(cancelled_jobs)
}

pub(crate) async fn job_details(
    tx: &DbTransaction<'_>,
    filters: JobFilters,
    order_by: JobOrderBy,
    page_size: u32,
    page_token: Option<String>,
) -> crate::Result<(Vec<JobDetails>, Option<NextPageToken>)> {
    let page_token = if let Some(page_token) = page_token {
        PageToken::from_str(&page_token)?
    } else {
        PageToken {
            last_job_id: None,
            last_target_execution_time: None,
            filters,
            order_by,
        }
    };

    let mut jobs: Vec<JobDetails> = {
        let mut select = Query::select()
            .from(("ora", "job"))
            .join_subquery(
                JoinType::Join,
                select_job_ids(&page_token.filters),
                "job_ids",
                Expr::col(("ora", "job", "id")).eq(Expr::col(("job_ids", "job_id"))),
            )
            .expr(Expr::col(("ora", "job", "id")))
            .expr(Expr::cust(
                "EXTRACT(EPOCH FROM ora.job.created_at)::DOUBLE PRECISION",
            ))
            .expr(Expr::col(("ora", "job", "job_type_id")))
            .expr(Expr::cust(
                "EXTRACT(EPOCH FROM ora.job.target_execution_time)::DOUBLE PRECISION",
            ))
            .expr(Expr::col(("ora", "job", "input_payload_json")))
            .expr(Expr::col(("ora", "job", "timeout_policy_json")))
            .expr(Expr::col(("ora", "job", "retry_policy_json")))
            .expr(Expr::col(("ora", "job", "schedule_id")))
            .limit(u64::from(page_size))
            .take();

        match page_token.order_by {
            JobOrderBy::TargetExecutionTimeAsc => {
                select.order_by_customs([
                    ("ora.job.target_execution_time", Order::Asc),
                    ("ora.job.id", Order::Asc),
                ]);

                if let (Some(last_target_execution_time), Some(last_job_id)) = (
                    page_token.last_target_execution_time,
                    page_token.last_job_id,
                ) {
                    select.and_where(Expr::cust_with_values(
                        r#"
                            (ora.job.target_execution_time, ora.job.id)
                                > (to_timestamp($1::DOUBLE PRECISION), $2::UUID)
                        "#,
                        [
                            Value::from(systemtime_to_ts(last_target_execution_time)),
                            last_job_id.0.into(),
                        ],
                    ));
                }
            }
            JobOrderBy::TargetExecutionTimeDesc => {
                select.order_by_customs([
                    ("ora.job.target_execution_time", Order::Desc),
                    ("ora.job.id", Order::Asc),
                ]);

                if let (Some(last_target_execution_time), Some(last_job_id)) = (
                    page_token.last_target_execution_time,
                    page_token.last_job_id,
                ) {
                    // The time is descending but ties are ordered by ascending IDs,
                    // so a row comparison doesn't work here.
                    select.and_where(Expr::cust_with_values(
                        r#"
                            (ora.job.target_execution_time < to_timestamp($1::DOUBLE PRECISION)
                                OR (ora.job.target_execution_time = to_timestamp($1::DOUBLE PRECISION)
                                    AND ora.job.id > $2::UUID))
                        "#,
                        [
                            Value::from(systemtime_to_ts(last_target_execution_time)),
                            last_job_id.0.into(),
                        ],
                    ));
                }
            }
            JobOrderBy::CreatedAtAsc => {
                select.order_by_customs([("ora.job.id", Order::Asc)]);

                if let Some(last_job_id) = page_token.last_job_id {
                    select.and_where(Expr::cust_with_values(
                        "ora.job.id > $1::UUID",
                        [last_job_id.0],
                    ));
                }
            }
            JobOrderBy::CreatedAtDesc => {
                select.order_by_customs([("ora.job.id", Order::Desc)]);

                if let Some(last_job_id) = page_token.last_job_id {
                    select.and_where(Expr::cust_with_values(
                        "ora.job.id < $1::UUID",
                        [last_job_id.0],
                    ));
                }
            }
        }

        let (query, values) = select.build_postgres(PostgresQueryBuilder);

        let stmt = tx.prepare_owned(query).await?;
        let rows = tx.query(&stmt, &values.as_params()).await?;

        let mut jobs = Vec::with_capacity(rows.len());

        for row in rows {
            jobs.push(JobDetails {
                id: JobId(row.try_get(0)?),
                created_at: systemtime_from_ts(row.try_get(1)?),
                job: JobDefinition {
                    job_type_id: JobTypeId::new_unchecked(row.try_get::<_, String>(2)?),
                    target_execution_time: systemtime_from_ts(row.try_get(3)?),
                    input_payload_json: row.try_get(4)?,
                    labels: Vec::new(),
                    timeout_policy: serde_json::from_str(row.try_get::<_, &str>(5)?)?,
                    retry_policy: serde_json::from_str(row.try_get::<_, &str>(6)?)?,
                },
                schedule_id: row.try_get::<_, Option<Uuid>>(7)?.map(ScheduleId),
                executions: Vec::new(),
            });
        }

        jobs
    };

    collect_labels(tx, &mut jobs).await?;
    collect_executions(tx, &mut jobs).await?;

    let mut page_token = page_token;
    let next_page_token = if let Some(last_job) = jobs.last() {
        page_token.last_job_id = Some(last_job.id);
        page_token.last_target_execution_time = Some(last_job.job.target_execution_time);
        Some(NextPageToken(page_token.to_string()))
    } else {
        None
    };

    Ok((jobs, next_page_token))
}

async fn collect_labels(tx: &DbTransaction<'_>, jobs: &mut [JobDetails]) -> crate::Result<()> {
    let job_ids = jobs.iter().map(|job| job.id.0).collect::<Vec<_>>();

    let stmt = tx
        .prepare(
            r#"--sql
            SELECT
                job_id,
                label_key,
                label_value
            FROM
                ora.job_label
            WHERE
                job_id = ANY($1)
            ORDER BY job_id ASC, label_key ASC
            "#,
        )
        .await?;

    let rows = tx.query(&stmt, &[&job_ids]).await?;

    for row in rows {
        let job_id: Uuid = row.try_get(0)?;
        let key: String = row.try_get(1)?;
        let value: String = row.try_get(2)?;

        let job = jobs
            .iter_mut()
            .find(|job| job.id.0 == job_id)
            // This should be impossible.
            .expect("label returned for unknown job");

        job.job.labels.push(Label { key, value });
    }

    Ok(())
}

async fn collect_executions(tx: &DbTransaction<'_>, jobs: &mut [JobDetails]) -> crate::Result<()> {
    let job_ids = jobs.iter().map(|job| job.id.0).collect::<Vec<_>>();

    let stmt = tx
        .prepare(
            r#"--sql
            SELECT
                job_id,
                id,
                EXTRACT(EPOCH FROM created_at)::DOUBLE PRECISION,
                EXTRACT(EPOCH FROM started_at)::DOUBLE PRECISION,
                EXTRACT(EPOCH FROM succeeded_at)::DOUBLE PRECISION,
                EXTRACT(EPOCH FROM failed_at)::DOUBLE PRECISION,
                EXTRACT(EPOCH FROM cancelled_at)::DOUBLE PRECISION,
                output_json,
                failure_reason,
                status,
                executor_id,
                EXTRACT(EPOCH FROM ora.execution.target_execution_time)::DOUBLE PRECISION
            FROM
                ora.execution
            WHERE
                job_id = ANY($1)
            ORDER BY id ASC
            "#,
        )
        .await?;

    let rows = tx.query(&stmt, &[&job_ids]).await?;

    for row in rows {
        let job_id: Uuid = row.try_get(0)?;

        let job = jobs
            .iter_mut()
            .find(|job| job.id.0 == job_id)
            // This should be impossible.
            .expect("label returned for unknown job");

        job.executions.push(ExecutionDetails {
            id: ExecutionId(row.try_get(1)?),
            created_at: systemtime_from_ts(row.try_get(2)?),
            started_at: row.try_get::<_, Option<f64>>(3)?.map(systemtime_from_ts),
            succeeded_at: row.try_get::<_, Option<f64>>(4)?.map(systemtime_from_ts),
            failed_at: row.try_get::<_, Option<f64>>(5)?.map(systemtime_from_ts),
            cancelled_at: row.try_get::<_, Option<f64>>(6)?.map(systemtime_from_ts),
            output_json: row.try_get(7)?,
            failure_reason: row.try_get(8)?,
            status: PgExecutionStatus::from(row.try_get::<_, i16>(9)?).into(),
            executor_id: row.try_get::<_, Option<Uuid>>(10)?.map(ExecutorId),
            target_execution_time: systemtime_from_ts(row.try_get(11)?),
        });
    }

    Ok(())
}

pub(crate) async fn job_count(tx: &DbTransaction<'_>, filters: JobFilters) -> crate::Result<i64> {
    let (mut select, executions_joined) = filtered_jobs(&filters);

    let (query, values) = select
        .expr(count_jobs(executions_joined))
        .build_postgres(PostgresQueryBuilder);

    let stmt = tx.prepare_owned(query).await?;
    let count: i64 = tx.query_one(&stmt, &values.as_params()).await?.get(0);
    Ok(count)
}

/// Counts the jobs of a query from [`filtered_jobs`].
fn count_jobs(executions_joined: bool) -> Expr {
    if executions_joined {
        // Jobs are repeated for each matching execution.
        Expr::cust("COUNT(DISTINCT ora.job.id)")
    } else {
        Expr::count(Expr::col(Asterisk))
    }
}

pub(crate) async fn job_ids(
    tx: &DbTransaction<'_>,
    filters: JobFilters,
) -> crate::Result<Vec<JobId>> {
    let (query, values) = select_job_ids(&filters).build_postgres(PostgresQueryBuilder);

    let stmt = tx.prepare_owned(query).await?;

    let rows = tx.query(&stmt, &values.as_params()).await?;

    let job_ids = rows
        .into_iter()
        .map(|row| Ok(JobId(row.try_get(0)?)))
        .collect::<Result<Vec<_>, crate::Error>>()?;

    Ok(job_ids)
}

fn select_job_ids(filters: &JobFilters) -> SelectStatement {
    let (mut select, executions_joined) = filtered_jobs(filters);

    select.expr_as(Expr::col(("ora", "job", "id")), "job_id");

    // Only the executions join can repeat a job ID. De-duplicating
    // sorts the whole matching set before any LIMIT applies, so one
    // page of a job type with a lot of history pays for all of it.
    if executions_joined {
        select.distinct();
    }

    select
}

/// Selects the jobs matching the filters from `ora.job`, without selecting any columns.
///
/// Also returns whether executions are joined, which repeats jobs for
/// each matching execution.
fn filtered_jobs(filters: &JobFilters) -> (SelectStatement, bool) {
    let JobFilters {
        job_ids,
        job_type_ids,
        executor_ids,
        target_execution_time,
        created_at,
        labels,
        execution_ids,
        execution_statuses,
        schedule_ids,
    } = filters;

    let mut select = SelectStatement::new().from(("ora", "job")).take();

    let mut executions_joined = false;

    let mut join_executions = |select: &mut SelectStatement| {
        if !executions_joined {
            select.join(
                JoinType::Join,
                ("ora", "execution"),
                Expr::col(("ora", "job", "id")).equals(("ora", "execution", "job_id")),
            );
        }
        executions_joined = true;
    };

    if let Some(job_ids) = job_ids {
        let job_ids: Vec<Uuid> = job_ids.iter().copied().map(Into::into).collect();

        select.and_where(Expr::cust_with_values(
            "ora.job.id = ANY($1::UUID[])",
            [job_ids],
        ));
    }

    if let Some(job_type_ids) = job_type_ids {
        let job_type_ids: Vec<String> = job_type_ids
            .iter()
            .cloned()
            .map(JobTypeId::into_inner)
            .collect();

        select.and_where(Expr::cust_with_values(
            "ora.job.job_type_id = ANY($1::TEXT[])",
            [job_type_ids],
        ));
    }

    if let Some(execution_ids) = execution_ids {
        join_executions(&mut select);

        let execution_ids: Vec<Uuid> = execution_ids.iter().copied().map(Into::into).collect();

        select.and_where(Expr::cust_with_values(
            "ora.execution.id = ANY($1::UUID[])",
            [execution_ids],
        ));
    }

    if let Some(executor_ids) = executor_ids {
        join_executions(&mut select);

        let executor_ids: Vec<Uuid> = executor_ids.iter().copied().map(Into::into).collect();

        select.and_where(Expr::cust_with_values(
            "ora.execution.executor_id = ANY($1::UUID[])",
            [executor_ids],
        ));
    }

    if let Some(target_execution_time) = target_execution_time {
        if let Some(start) = target_execution_time.start {
            let start = systemtime_to_ts(start);

            select.and_where(Expr::col(("ora", "job", "target_execution_time")).gte(
                Expr::cust_with_values("to_timestamp($1::DOUBLE PRECISION)", [start]),
            ));
        }

        if let Some(end) = target_execution_time.end {
            let end = systemtime_to_ts(end);

            select.and_where(Expr::col(("ora", "job", "target_execution_time")).lt(
                Expr::cust_with_values("to_timestamp($1::DOUBLE PRECISION)", [end]),
            ));
        }
    }

    if let Some(created_at) = created_at {
        if let Some(start) = created_at.start {
            let start = systemtime_to_ts(start);

            select.and_where(
                Expr::col(("ora", "job", "created_at")).gte(Expr::cust_with_values(
                    "to_timestamp($1::DOUBLE PRECISION)",
                    [start],
                )),
            );
        }

        if let Some(end) = created_at.end {
            let end = systemtime_to_ts(end);

            select.and_where(
                Expr::col(("ora", "job", "created_at")).lt(Expr::cust_with_values(
                    "to_timestamp($1::DOUBLE PRECISION)",
                    [end],
                )),
            );
        }
    }

    if let Some(statuses) = execution_statuses {
        select.and_where(status_filter(statuses));
    }

    if let Some(labels) = labels {
        for label_filter in labels {
            let mut subquery = SelectStatement::new()
                .from(("ora", "job_label"))
                .and_where(
                    Expr::col(("ora", "job_label", "job_id")).eq(Expr::col(("ora", "job", "id"))),
                )
                .and_where(
                    Expr::col(("ora", "job_label", "label_key"))
                        .eq(Expr::cust_with_values("$1", [label_filter.key.clone()])),
                )
                .take();

            if let Some(value) = label_filter.value.clone() {
                subquery.and_where(
                    Expr::col(("ora", "job_label", "label_value"))
                        .eq(Expr::cust_with_values("$1", [value])),
                );
            }

            select.and_where(Expr::exists(subquery));
        }
    }

    if let Some(schedule_ids) = schedule_ids {
        let schedule_ids: Vec<Uuid> = schedule_ids.iter().copied().map(Into::into).collect();

        select.and_where(Expr::cust_with_values(
            "ora.job.schedule_id = ANY($1::UUID[])",
            [schedule_ids],
        ));
    }

    (select, executions_joined)
}

/// Filters jobs by the status of their latest execution.
fn status_filter(statuses: &[ExecutionStatus]) -> Expr {
    let active_statuses = [ExecutionStatus::Pending, ExecutionStatus::InProgress];
    let inactive_statuses = [
        ExecutionStatus::Succeeded,
        ExecutionStatus::Failed,
        ExecutionStatus::Cancelled,
    ];

    let status_values = |candidates: &[ExecutionStatus]| -> Vec<i16> {
        candidates
            .iter()
            .filter(|s| statuses.contains(s))
            .map(|s| PgExecutionStatus::from(*s) as i16)
            .collect()
    };

    let active_values = status_values(&active_statuses);
    let inactive_values = status_values(&inactive_statuses);

    if active_values.len() == active_statuses.len()
        && inactive_values.len() == inactive_statuses.len()
    {
        return Expr::cust("TRUE");
    }

    // Active jobs have no stored status, their executions are only needed
    // if a subset of the active statuses is requested.
    let active = match active_values.len() {
        0 => None,
        n if n == active_statuses.len() => {
            Some(Expr::col(("ora", "job", "inactive_status")).is_null())
        }
        _ => Some(Expr::col(("ora", "job", "inactive_status")).is_null().and(
            Expr::cust_with_values(
                "ora.job.id IN (SELECT job_id FROM ora.execution WHERE status = ANY($1::SMALLINT[]))",
                [active_values],
            ),
        )),
    };

    let inactive = match inactive_values.len() {
        0 => None,
        n if n == inactive_statuses.len() => {
            Some(Expr::col(("ora", "job", "inactive_status")).is_not_null())
        }
        _ => Some(Expr::cust_with_values(
            "ora.job.inactive_status = ANY($1::SMALLINT[])",
            [inactive_values],
        )),
    };

    match (active, inactive) {
        (Some(active), Some(inactive)) => active.or(inactive),
        (Some(filter), None) | (None, Some(filter)) => filter,
        // No statuses match nothing.
        (None, None) => Expr::cust("FALSE"),
    }
}

#[derive(Debug, Serialize, Deserialize)]
struct PageToken {
    pub(super) last_job_id: Option<JobId>,
    pub(super) last_target_execution_time: Option<SystemTime>,
    pub(super) filters: JobFilters,
    pub(super) order_by: JobOrderBy,
}

impl FromStr for PageToken {
    type Err = crate::Error;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let decoded = base64::engine::general_purpose::URL_SAFE_NO_PAD
            .decode(s)
            .map_err(|err| crate::Error::InvalidPageToken(Box::new(err)))?;
        let cursor: PageToken = serde_json::from_slice(&decoded)
            .map_err(|err| crate::Error::InvalidPageToken(Box::new(err)))?;
        Ok(cursor)
    }
}

impl core::fmt::Display for PageToken {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        let json = serde_json::to_vec(self).map_err(|_| core::fmt::Error)?;
        let encoded = base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(json);
        f.write_str(&encoded)
    }
}
