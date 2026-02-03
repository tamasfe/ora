use std::{str::FromStr, time::UNIX_EPOCH};

use base64::Engine;
use ora_backend::{
    common::{Label, NextPageToken, TimeRange},
    jobs::JobTypeId,
    schedules::{
        ScheduleDefinition, ScheduleDetails, ScheduleFilters, ScheduleId, ScheduleOrderBy,
        ScheduleStatus, StoppedSchedule,
    },
};
use sea_query::{
    Asterisk, Expr, ExprTrait, JoinType, Order, PostgresQueryBuilder, Query, SelectStatement,
};
use sea_query_postgres::PostgresBinder;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::db::DbTransaction;

pub(crate) async fn stop_schedules(
    tx: &DbTransaction<'_>,
    mut filters: ScheduleFilters,
) -> crate::Result<Vec<StoppedSchedule>> {
    filters.statuses = Some(vec![ScheduleStatus::Active]);

    let (query, values) = Query::update()
        .table(("ora", "schedule"))
        .value("stopped_at", Expr::cust("NOW()"))
        .and_where(Expr::col(("ora", "schedule", "id")).in_subquery(select_schedule_ids(&filters)))
        .returning(sea_query::ReturningClause::Columns(
            vec![("ora", "schedule", "id").into()],
        ))
        .build_postgres(PostgresQueryBuilder);

    let stmt = tx.prepare_owned(query).await?;

    let rows = tx.query(&stmt, &values.as_params()).await?;

    let mut cancelled_jobs = Vec::with_capacity(rows.len());

    for row in rows {
        let schedule_id = ScheduleId(row.try_get(0)?);

        cancelled_jobs.push(StoppedSchedule { schedule_id });
    }

    Ok(cancelled_jobs)
}

pub(crate) async fn schedule_details(
    tx: &DbTransaction<'_>,
    filters: ScheduleFilters,
    order_by: ScheduleOrderBy,
    page_size: u32,
    page_token: Option<String>,
) -> crate::Result<(Vec<ScheduleDetails>, Option<NextPageToken>)> {
    let page_token = if let Some(page_token) = page_token {
        PageToken::from_str(&page_token)?
    } else {
        PageToken {
            last_schedule_id: None,
            filters,
            order_by,
        }
    };

    let mut select = Query::select()
        .from(("ora", "schedule"))
        .join_subquery(
            JoinType::Join,
            select_schedule_ids(&page_token.filters),
            "schedule_ids",
            Expr::col(("ora", "schedule", "id")).eq(Expr::col(("schedule_ids", "schedule_id"))),
        )
        .expr(Expr::col(("ora", "schedule", "id")))
        .expr(Expr::col(("ora", "schedule", "scheduling_policy_json")))
        .expr(Expr::col(("ora", "schedule", "job_template_json")))
        .expr(Expr::cust(
            "EXTRACT(EPOCH FROM ora.schedule.start_after)::DOUBLE PRECISION",
        ))
        .expr(Expr::cust(
            "EXTRACT(EPOCH FROM ora.schedule.end_before)::DOUBLE PRECISION",
        ))
        .expr(Expr::cust(
            "EXTRACT(EPOCH FROM ora.schedule.created_at)::DOUBLE PRECISION",
        ))
        .expr(Expr::cust(
            "EXTRACT(EPOCH FROM ora.schedule.stopped_at)::DOUBLE PRECISION",
        ))
        .limit(u64::from(page_size))
        .take();

    match page_token.order_by {
        ScheduleOrderBy::CreatedAtAsc => {
            select.order_by_customs([("ora.schedule.id", Order::Asc)]);

            if let Some(last_schedule_id) = page_token.last_schedule_id {
                select.and_where(Expr::cust_with_values(
                    "ora.schedule.id > $1::UUID",
                    [last_schedule_id.0],
                ));
            }
        }
        ScheduleOrderBy::CreatedAtDesc => {
            select.order_by_customs([("ora.schedule.id", Order::Desc)]);

            if let Some(last_schedule_id) = page_token.last_schedule_id {
                select.and_where(Expr::cust_with_values(
                    "ora.schedule.id < $1::UUID",
                    [last_schedule_id.0],
                ));
            }
        }
    }

    let (query, values) = select.build_postgres(PostgresQueryBuilder);

    let stmt = tx.prepare_owned(query).await?;
    let rows = tx.query(&stmt, &values.as_params()).await?;

    let mut schedules = Vec::with_capacity(rows.len());

    for row in rows {
        let stopped_at = row
            .try_get::<_, Option<f64>>(6)?
            .map(|ts| UNIX_EPOCH + std::time::Duration::from_secs_f64(ts));

        schedules.push(ScheduleDetails {
            id: ScheduleId(row.try_get(0)?),
            schedule: ScheduleDefinition {
                scheduling: serde_json::from_str(row.try_get::<_, &str>(1)?)?,
                job_template: serde_json::from_str(row.try_get::<_, &str>(2)?)?,
                labels: Vec::new(),
                time_range: TimeRange {
                    start: row
                        .try_get::<_, Option<f64>>(3)?
                        .map(|ts| UNIX_EPOCH + std::time::Duration::from_secs_f64(ts)),
                    end: row
                        .try_get::<_, Option<f64>>(4)?
                        .map(|ts| UNIX_EPOCH + std::time::Duration::from_secs_f64(ts)),
                },
            },
            created_at: UNIX_EPOCH + std::time::Duration::from_secs_f64(row.try_get::<_, f64>(5)?),
            status: if stopped_at.is_some() {
                ScheduleStatus::Stopped
            } else {
                ScheduleStatus::Active
            },
            stopped_at,
        });
    }

    collect_labels(tx, &mut schedules).await?;

    let mut page_token = page_token;
    let next_page_token = if let Some(last_schedule) = schedules.last() {
        page_token.last_schedule_id = Some(last_schedule.id);
        Some(NextPageToken(page_token.to_string()))
    } else {
        None
    };

    Ok((schedules, next_page_token))
}

async fn collect_labels(
    tx: &DbTransaction<'_>,
    schedules: &mut [ScheduleDetails],
) -> crate::Result<()> {
    let schedule_ids = schedules
        .iter()
        .map(|schedule| schedule.id.0)
        .collect::<Vec<_>>();

    let stmt = tx
        .prepare(
            r#"--sql
            SELECT
                schedule_id,
                label_key,
                label_value
            FROM
                ora.schedule_label
            WHERE
                schedule_id = ANY($1)
            ORDER BY schedule_id ASC, label_key ASC
            "#,
        )
        .await?;

    let rows = tx.query(&stmt, &[&schedule_ids]).await?;

    for row in rows {
        let schedule_id: Uuid = row.try_get(0)?;
        let key: String = row.try_get(1)?;
        let value: String = row.try_get(2)?;

        let schedule = schedules
            .iter_mut()
            .find(|schedule| schedule.id.0 == schedule_id)
            // This should be impossible.
            .expect("label returned for unknown job");

        schedule.schedule.labels.push(Label { key, value });
    }

    Ok(())
}

pub(crate) async fn schedule_count(
    tx: &DbTransaction<'_>,
    filters: ScheduleFilters,
) -> crate::Result<i64> {
    let (query, values) = select_schedule_ids(&filters)
        .clear_selects()
        .expr(Expr::count(Expr::col(Asterisk)))
        .build_postgres(PostgresQueryBuilder);

    let stmt = tx.prepare_owned(query).await?;
    let count: i64 = tx.query_one(&stmt, &values.as_params()).await?.try_get(0)?;
    Ok(count)
}

pub(crate) async fn schedule_exists(
    tx: &DbTransaction<'_>,
    filters: ScheduleFilters,
) -> crate::Result<bool> {
    let (query, values) = SelectStatement::new()
        .expr(Expr::exists(select_schedule_ids(&filters)))
        .take()
        .build_postgres(PostgresQueryBuilder);

    let stmt = tx.prepare_owned(query).await?;

    let exists: bool = tx.query_one(&stmt, &values.as_params()).await?.try_get(0)?;

    Ok(exists)
}

pub(super) fn select_schedule_ids(filters: &ScheduleFilters) -> SelectStatement {
    let ScheduleFilters {
        schedule_ids,
        job_type_ids,
        statuses,
        created_at,
        labels,
    } = filters;

    let mut select = SelectStatement::new()
        .distinct()
        .expr_as(Expr::col(("ora", "schedule", "id")), "schedule_id")
        .from(("ora", "schedule"))
        .take();

    if let Some(schedule_ids) = schedule_ids {
        let schedule_ids: Vec<Uuid> = schedule_ids.iter().copied().map(Into::into).collect();

        select.and_where(Expr::cust_with_values(
            "ora.schedule.id = ANY($1::UUID[])",
            [schedule_ids],
        ));
    }

    if let Some(job_type_ids) = job_type_ids {
        let job_type_ids: Vec<String> = job_type_ids
            .iter()
            .cloned()
            .map(JobTypeId::into_inner)
            .collect();

        select.and_where(Expr::cust_with_values(
            "ora.schedule.job_template_job_type_id = ANY($1::TEXT[])",
            [job_type_ids],
        ));
    }

    if let Some(created_at) = created_at {
        if let Some(start) = created_at.start {
            let start = start
                .duration_since(UNIX_EPOCH)
                .unwrap_or_default()
                .as_secs_f64();

            select.and_where(Expr::col(("ora", "schedule", "created_at")).gte(
                Expr::cust_with_values("to_timestamp($1::DOUBLE PRECISION)", [start]),
            ));
        }

        if let Some(end) = created_at.end {
            let end = end
                .duration_since(UNIX_EPOCH)
                .unwrap_or_default()
                .as_secs_f64();

            select.and_where(Expr::col(("ora", "schedule", "created_at")).lt(
                Expr::cust_with_values("to_timestamp($1::DOUBLE PRECISION)", [end]),
            ));
        }
    }

    if let Some(labels) = labels {
        for label_filter in labels {
            let mut subquery = SelectStatement::new()
                .from(("ora", "schedule_label"))
                .and_where(
                    Expr::col(("ora", "schedule_label", "schedule_id"))
                        .eq(Expr::col(("ora", "schedule", "id"))),
                )
                .and_where(
                    Expr::col(("ora", "schedule_label", "label_key"))
                        .eq(Expr::cust_with_values("$1", [label_filter.key.clone()])),
                )
                .take();

            if let Some(value) = label_filter.value.clone() {
                subquery.and_where(
                    Expr::col(("ora", "schedule_label", "label_value"))
                        .eq(Expr::cust_with_values("$1", [value])),
                );
            }

            select.and_where(Expr::exists(subquery));
        }
    }

    if let Some(statuses) = statuses {
        for status in statuses {
            match status {
                ScheduleStatus::Active => {
                    select.and_where(Expr::col(("ora", "schedule", "stopped_at")).is_null());
                }
                ScheduleStatus::Stopped => {
                    select.and_where(Expr::col(("ora", "schedule", "stopped_at")).is_not_null());
                }
            }
        }
    }

    select
}

#[derive(Debug, Serialize, Deserialize)]
struct PageToken {
    pub(super) last_schedule_id: Option<ScheduleId>,
    pub(super) filters: ScheduleFilters,
    pub(super) order_by: ScheduleOrderBy,
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
