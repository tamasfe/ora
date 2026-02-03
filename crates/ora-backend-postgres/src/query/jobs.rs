use std::{
    str::FromStr,
    time::{Duration, SystemTime, UNIX_EPOCH},
};

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

use crate::{db::DbTransaction, models::PgExecutionStatus};

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
                            (ora.job.target_execution_time >= to_timestamp($1::DOUBLE PRECISION)
                                AND ora.job.id > $2::UUID)
                        "#,
                        [
                            Value::from(
                                last_target_execution_time
                                    .duration_since(UNIX_EPOCH)
                                    .unwrap_or_default()
                                    .as_secs_f64(),
                            ),
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
                    select.and_where(Expr::cust_with_values(
                        r#"
                            (ora.job.target_execution_time <= to_timestamp($1::DOUBLE PRECISION)
                                AND ora.job.id > $2::UUID)
                        "#,
                        [
                            Value::from(
                                last_target_execution_time
                                    .duration_since(UNIX_EPOCH)
                                    .unwrap_or_default()
                                    .as_secs_f64(),
                            ),
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
                created_at: UNIX_EPOCH + Duration::from_secs_f64(row.try_get(1)?),
                job: JobDefinition {
                    job_type_id: JobTypeId::new_unchecked(row.try_get::<_, String>(2)?),
                    target_execution_time: UNIX_EPOCH + Duration::from_secs_f64(row.try_get(3)?),
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
                executor_id
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
            created_at: UNIX_EPOCH + Duration::from_secs_f64(row.try_get(2)?),
            started_at: row
                .try_get::<_, Option<f64>>(3)?
                .map(|d| UNIX_EPOCH + Duration::from_secs_f64(d)),
            succeeded_at: row
                .try_get::<_, Option<f64>>(4)?
                .map(|d| UNIX_EPOCH + Duration::from_secs_f64(d)),
            failed_at: row
                .try_get::<_, Option<f64>>(5)?
                .map(|d| UNIX_EPOCH + Duration::from_secs_f64(d)),
            cancelled_at: row
                .try_get::<_, Option<f64>>(6)?
                .map(|d| UNIX_EPOCH + Duration::from_secs_f64(d)),
            output_json: row.try_get(7)?,
            failure_reason: row.try_get(8)?,
            status: PgExecutionStatus::from(row.try_get::<_, i16>(9)?).into(),
            executor_id: row.try_get::<_, Option<Uuid>>(10)?.map(ExecutorId),
        });
    }

    Ok(())
}

pub(crate) async fn job_count(tx: &DbTransaction<'_>, filters: JobFilters) -> crate::Result<i64> {
    let (query, values) = select_job_ids(&filters)
        .clear_selects()
        .expr(Expr::count(Expr::col(Asterisk)))
        .build_postgres(PostgresQueryBuilder);

    let stmt = tx.prepare_owned(query).await?;
    let count: i64 = tx.query_one(&stmt, &values.as_params()).await?.get(0);
    Ok(count)
}

pub(crate) async fn job_exists(tx: &DbTransaction<'_>, filters: JobFilters) -> crate::Result<bool> {
    let (query, values) = SelectStatement::new()
        .expr(Expr::exists(select_job_ids(&filters)))
        .take()
        .build_postgres(PostgresQueryBuilder);

    let stmt = tx.prepare_owned(query).await?;

    let exists: bool = tx.query_one(&stmt, &values.as_params()).await?.get(0);

    Ok(exists)
}

fn select_job_ids(filters: &JobFilters) -> SelectStatement {
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

    let mut select = SelectStatement::new()
        .distinct()
        .expr_as(Expr::col(("ora", "job", "id")), "job_id")
        .from(("ora", "job"))
        .take();

    let mut join_executions = {
        let mut executions_joined = false;
        move |select: &mut SelectStatement| {
            if !executions_joined {
                select.join(
                    JoinType::Join,
                    ("ora", "execution"),
                    Expr::col(("ora", "job", "id")).equals(("ora", "execution", "job_id")),
                );
            }
            executions_joined = true;
        }
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
            let start = start
                .duration_since(UNIX_EPOCH)
                .unwrap_or_default()
                .as_secs_f64();

            select.and_where(Expr::col(("ora", "job", "target_execution_time")).gte(
                Expr::cust_with_values("to_timestamp($1::DOUBLE PRECISION)", [start]),
            ));
        }

        if let Some(end) = target_execution_time.end {
            let end = end
                .duration_since(UNIX_EPOCH)
                .unwrap_or_default()
                .as_secs_f64();

            select.and_where(Expr::col(("ora", "job", "target_execution_time")).lt(
                Expr::cust_with_values("to_timestamp($1::DOUBLE PRECISION)", [end]),
            ));
        }
    }

    if let Some(created_at) = created_at {
        if let Some(start) = created_at.start {
            let start = start
                .duration_since(UNIX_EPOCH)
                .unwrap_or_default()
                .as_secs_f64();

            select.and_where(
                Expr::col(("ora", "job", "created_at")).gte(Expr::cust_with_values(
                    "to_timestamp($1::DOUBLE PRECISION)",
                    [start],
                )),
            );
        }

        if let Some(end) = created_at.end {
            let end = end
                .duration_since(UNIX_EPOCH)
                .unwrap_or_default()
                .as_secs_f64();

            select.and_where(
                Expr::col(("ora", "job", "created_at")).lt(Expr::cust_with_values(
                    "to_timestamp($1::DOUBLE PRECISION)",
                    [end],
                )),
            );
        }
    }

    if let Some(statuses) = execution_statuses {
        let status_values: Vec<i16> = statuses.iter().map(|s| *s as i16).collect();

        select.and_where(Expr::cust_with_values(
            r#"
            (
                SELECT
                    status
                FROM
                    ora.execution
                WHERE
                    ora.execution.job_id = ora.job.id
                ORDER BY ora.execution.id DESC
                LIMIT 1
            ) = ANY($1::SMALLINT[])"#,
            [status_values],
        ));
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

    select
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
