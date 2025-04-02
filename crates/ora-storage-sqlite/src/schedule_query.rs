use core::fmt::Write;

use crate::sea_query_binder::RusqliteBinder;
use ora_storage::{
    ScheduleQueryFilters, ScheduleQueryOrder, ScheduleQueryResult, ScheduleTimeRange,
};
use rusqlite::Transaction;
use sea_query::{Expr, JoinType, Query, SelectStatement, SqliteQueryBuilder};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::models::SqlSystemTime;

pub(super) fn count_schedules(
    tx: &mut Transaction,
    filters: ScheduleQueryFilters,
) -> eyre::Result<u64> {
    let mut select_query = Query::select();
    select_query
        .expr(Expr::cust("COUNT(ora_schedule.id)"))
        .from(I("ora_schedule"));

    filter_schedules_query(&mut select_query, filters, None)?;

    let (query, values) = select_query.build_rusqlite(SqliteQueryBuilder);

    let mut statement = tx.prepare(&query)?;
    let mut rows = statement.query(&*values.as_params())?;

    let count: i64 = rows.next()?.unwrap().get(0)?;

    Ok(u64::try_from(count).unwrap())
}

pub(super) fn schedule_ids(
    tx: &mut Transaction,
    filters: ScheduleQueryFilters,
) -> eyre::Result<Vec<Uuid>> {
    let mut select_query = Query::select();
    select_query
        .column((I("ora_schedule"), I("id")))
        .from(I("ora_schedule"));
    filter_schedules_query(&mut select_query, filters, None)?;
    select_query.order_by((I("ora_schedule"), I("id")), sea_query::Order::Asc);

    let (query, values) = select_query.build_rusqlite(SqliteQueryBuilder);

    let mut statement = tx.prepare(&query)?;
    let mut rows = statement.query(&*values.as_params())?;

    let mut schedule_ids = Vec::new();
    while let Some(row) = rows.next()? {
        schedule_ids.push(row.get(0)?);
    }

    Ok(schedule_ids)
}

pub(super) fn delete_schedules(
    tx: &mut Transaction,
    filters: ScheduleQueryFilters,
) -> eyre::Result<Vec<Uuid>> {
    tx.execute(
        "CREATE TEMP TABLE IF NOT EXISTS temp.query_schedules(id BLOB, PRIMARY KEY(id))",
        [],
    )?;

    {
        let mut select_query = Query::select();
        select_query
            .column((I("ora_schedule"), I("id")))
            .from(I("ora_schedule"));

        filter_schedules_query(&mut select_query, filters, None)?;

        let (query, values) = Query::insert()
            .into_table((I("temp"), I("query_schedules")))
            .columns([I("id")])
            .select_from(select_query)?
            .build_rusqlite(SqliteQueryBuilder);

        let mut statement = tx.prepare(&query)?;
        statement.execute(&*values.as_params())?;
    }

    let deleted_schedules: Vec<Uuid> = tx
        .prepare(
            r#"--sql
            DELETE FROM
                "ora_schedule"
            WHERE
                EXISTS (
                    SELECT
                        1
                    FROM
                        temp.query_schedules
                    WHERE
                        "ora_schedule"."id" = temp.query_schedules."id"
                )
            RETURNING "id"
        "#,
        )?
        .query_map([], |row| row.get(0))?
        .collect::<Result<Vec<_>, _>>()?;

    tx.execute(
        r#"--sql
        DELETE FROM
            "ora_schedule_label"
        WHERE
            EXISTS (
                SELECT
                    1
                FROM
                    temp.query_schedules
                WHERE
                    "ora_schedule_label"."schedule_id" = temp.query_schedules."id"
            )
        "#,
        [],
    )?;

    tx.execute("DROP TABLE temp.query_schedules", [])?;

    Ok(deleted_schedules)
}

pub(super) fn query_schedule_details(
    tx: &mut Transaction,
    cursor: Option<Cursor>,
    limit: usize,
    order: ScheduleQueryOrder,
    filters: ScheduleQueryFilters,
) -> eyre::Result<ScheduleQueryResult> {
    let (last_schedule_id, filters, order) = match cursor {
        Some(cursor) => (cursor.last_schedule_id, cursor.filters, cursor.order),
        None => (None, filters, order),
    };

    tx.execute(
        "CREATE TEMP TABLE IF NOT EXISTS temp.query_schedules(id BLOB)",
        [],
    )?;

    {
        let mut select_query = Query::select();
        select_query
            .column((I("ora_schedule"), I("id")))
            .from(I("ora_schedule"));

        filter_schedules_query(&mut select_query, filters.clone(), last_schedule_id)?;
        order_schedules_query(&mut select_query, order);

        let (query, values) = Query::insert()
            .into_table((I("temp"), I("query_schedules")))
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
            temp.query_schedules
        "#,
        [],
        |row| row.get(0),
    )?;

    let mut schedules = Vec::new();

    {
        let mut stmt = tx.prepare_cached(
            r#"--sql
            SELECT
                "ora_schedule"."id",
                "created_at_unix_ns",
                "job_timing_policy",
                "job_creation_policy",
                (
                    "marked_unschedulable_at_unix_ns" IS NULL
                    OR "ora_schedule_job_state"."active_job_id" IS NOT NULL
                ) AS "active",
                "cancelled_at_unix_ns",
                "start_after_unix_ns",
                "end_before_unix_ns",
                "metadata_json"
            FROM
                "ora_schedule"
            JOIN "ora_schedule_job_state" ON
                "ora_schedule"."id" = "ora_schedule_job_state"."schedule_id"
            JOIN temp.query_schedules ON
                "ora_schedule"."id" = temp.query_schedules."id"
            LIMIT ?
            "#,
        )?;

        let mut rows = stmt.query([limit])?;

        while let Some(row) = rows.next()? {
            let id: Uuid = row.get(0)?;
            let created_at: SqlSystemTime = row.get(1)?;
            let job_timing_policy: crate::models::ScheduleJobTimingPolicy = row.get(2)?;
            let job_creation_policy: crate::models::ScheduleJobCreationPolicy = row.get(3)?;
            let active: bool = row.get(4)?;
            let cancelled_at: Option<SqlSystemTime> = row.get(5)?;
            let start_after: Option<SqlSystemTime> = row.get(6)?;
            let end_before: Option<SqlSystemTime> = row.get(7)?;
            let metadata_json: Option<String> = row.get(8)?;

            schedules.push(ora_storage::ScheduleDetails {
                id,
                created_at: created_at.into(),
                job_timing_policy: job_timing_policy.into(),
                job_creation_policy: job_creation_policy.into(),
                active,
                cancelled: cancelled_at.is_some(),
                time_range: Some(ScheduleTimeRange {
                    start: start_after.map(Into::into),
                    end: end_before.map(Into::into),
                }),
                metadata_json,
                labels: Default::default(),
            });
        }
    }

    {
        let mut stmt = tx.prepare_cached(
            r#"--sql
            SELECT
                "schedule_id",
                "key",
                "value"
            FROM
                "ora_schedule_label"
            JOIN
                temp.query_schedules
            ON
                "ora_schedule_label"."schedule_id" = temp.query_schedules."id"
            "#,
        )?;

        let mut rows = stmt.query([])?;

        while let Some(row) = rows.next()? {
            let schedule_id: Uuid = row.get(0)?;
            let key: String = row.get(1)?;
            let value: String = row.get(2)?;

            if let Some(schedule) = schedules
                .iter_mut()
                .find(|schedule| schedule.id == schedule_id)
            {
                schedule.labels.insert(key, value);
            }
        }
    }

    tx.execute("DROP TABLE temp.query_schedules", [])?;

    let new_cursor = Cursor {
        last_schedule_id: schedules.last().map(|schedule| schedule.id),
        filters,
        order,
    };

    Ok(ScheduleQueryResult {
        has_more: schedules.len() < total_count,
        cursor: Some(serde_json::to_string(&new_cursor).unwrap()),
        schedules,
    })
}

fn filter_schedules_query(
    query: &mut SelectStatement,
    filters: ScheduleQueryFilters,
    after_id: Option<Uuid>,
) -> eyre::Result<()> {
    let ScheduleQueryFilters {
        job_ids,
        job_type_ids,
        schedule_ids,
        labels,
        active,
        created_after,
        created_before,
    } = filters;

    let job_state_joined = false;
    let jobs_joined = false;

    if let Some(after_id) = after_id {
        query.and_where(
            Expr::col((I("ora_schedule"), I("id")))
                .gt(sea_query::Value::Uuid(Some(Box::new(after_id)))),
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

    if let Some(schedule_ids) = schedule_ids {
        query.and_where(
            Expr::col((I("ora_schedule"), I("id"))).is_in(
                schedule_ids
                    .into_iter()
                    .map(|id| sea_query::Value::Uuid(Some(Box::new(id)))),
            ),
        );
    }

    if let Some(job_ids) = job_ids {
        if !jobs_joined {
            query.join(
                JoinType::Join,
                I("ora_job"),
                Expr::col((I("ora_schedule"), I("id"))).equals((I("ora_job"), I("schedule_id"))),
            );
        }

        query.and_where(
            Expr::col((I("ora_job"), I("id"))).is_in(
                job_ids
                    .into_iter()
                    .map(|id| sea_query::Value::Uuid(Some(Box::new(id)))),
            ),
        );
    }

    if let Some(labels) = labels {
        for (key, value) in labels {
            match value {
                ora_storage::ScheduleLabelFilterValue::Exists => {
                    query.and_where(Expr::cust_with_values(
                        r#"--sql
                                EXISTS
                                (
                                    SELECT
                                        1
                                    FROM
                                        "ora_schedule_label"
                                    WHERE
                                        "schedule_id" = "ora_schedule"."id"
                                        AND "key" = ?
                                )
                                "#,
                        [sea_query::Value::String(Some(Box::new(key)))],
                    ));
                }
                ora_storage::ScheduleLabelFilterValue::Equals(value) => {
                    query.and_where(Expr::cust_with_values(
                        r#"--sql
                                EXISTS
                                (
                                    SELECT
                                        1
                                    FROM
                                        "ora_schedule_label"
                                    WHERE
                                        "schedule_id" = "ora_schedule"."id"
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
        if !job_state_joined {
            query.join(
                JoinType::Join,
                I("ora_schedule_job_state"),
                Expr::col((I("ora_schedule"), I("id")))
                    .equals((I("ora_schedule_job_state"), I("schedule_id"))),
            );
        }

        if active {
            query.and_where(Expr::cust(
                r#"--sql
                    (
                        "ora_schedule"."marked_unschedulable_at_unix_ns" IS NULL
                        OR "ora_schedule_job_state"."active_job_id" IS NOT NULL
                    )
                    "#,
            ));
        } else {
            query.and_where(Expr::cust(
                r#"--sql
                    (
                        "ora_schedule"."marked_unschedulable_at_unix_ns" IS NOT NULL
                        AND "ora_schedule_job_state"."active_job_id" IS NULL
                    )
                    "#,
            ));
        }
    }

    if let Some(created_after) = created_after {
        query.and_where(
            Expr::col((I("ora_schedule"), I("created_at_unix_ns")))
                .gte(SqlSystemTime(created_after).as_i64()?),
        );
    }

    if let Some(created_before) = created_before {
        query.and_where(
            Expr::col((I("ora_schedule"), I("created_at_unix_ns")))
                .lt(SqlSystemTime(created_before).as_i64()?),
        );
    }

    Ok(())
}

fn order_schedules_query(query: &mut SelectStatement, order: ScheduleQueryOrder) {
    match order {
        ScheduleQueryOrder::CreatedAtAsc => {
            query.order_by((I("ora_schedule"), I("id")), sea_query::Order::Asc);
        }
        ScheduleQueryOrder::CreatedAtDesc => {
            query.order_by((I("ora_schedule"), I("id")), sea_query::Order::Desc);
        }
    }
}

#[derive(Debug, Serialize, Deserialize)]
pub(super) struct Cursor {
    pub(super) last_schedule_id: Option<Uuid>,
    pub(super) filters: ScheduleQueryFilters,
    pub(super) order: ScheduleQueryOrder,
}

struct I(&'static str);

impl sea_query::Iden for I {
    fn unquoted(&self, s: &mut dyn Write) {
        s.write_str(self.0).unwrap();
    }
}
