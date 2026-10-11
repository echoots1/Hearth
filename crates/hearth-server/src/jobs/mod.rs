//! In-process jobs: a scheduler, advisory locks, `job_runs` rows.
//!
//! Jobs detect and compute; they never decide (ARCHITECTURE §7). Each job
//! takes a Postgres advisory lock so two containers never run it at once,
//! and records a `job_runs` row with a summary.

pub mod evaluate;
pub mod housekeeping;
pub mod notify;
pub mod process;

use std::time::Duration;

use chrono::{DateTime, Utc};
use serde::Serialize;
use serde_json::Value;
use sqlx::PgPool;

use crate::api::AppState;
use crate::db::models::{JobRun, JobRunStatus};
use crate::services::Actor;
use crate::tools::BoxFut;

pub type Handler = fn(AppState) -> BoxFut<anyhow::Result<Value>>;

pub struct Job {
    pub name: &'static str,
    pub description: &'static str,
    pub every: Duration,
    pub handler: Handler,
}

pub fn actor(name: &str) -> Actor {
    Actor::Job(name.to_string())
}

/// Every job this build knows, with its default interval. Sync jobs are
/// added by the integrations when they are configured.
pub fn registry(state: &AppState) -> Vec<Job> {
    let j = &state.config.jobs;
    let mut v = vec![
        Job {
            name: "process",
            description: "apply rules, match bills and planned items, shadow-link card duplicates, queue the rest for review",
            every: Duration::from_secs(5 * 60),
            handler: |s| Box::pin(process::run(s)),
        },
        Job {
            name: "evaluate",
            description: "recompute the month and the low point; raise or refresh alerts by dedupe key; note milestones",
            every: Duration::from_secs(j.evaluate_minutes.max(1) * 60),
            handler: |s| Box::pin(evaluate::run(s)),
        },
        Job {
            name: "notify",
            description: "send new alerts, questions and proposals; nag unacknowledged alerts; honor quiet hours",
            every: Duration::from_secs(60),
            handler: |s| Box::pin(notify::run(s)),
        },
        Job {
            name: "housekeeping",
            description: "prune old job runs, expired sessions, dead push subscriptions and old notification rows",
            every: Duration::from_secs(24 * 60 * 60),
            handler: |s| Box::pin(housekeeping::run(s)),
        },
    ];
    v.extend(crate::integrations::jobs(state));
    v
}

pub fn get(state: &AppState, name: &str) -> Option<Job> {
    registry(state).into_iter().find(|j| j.name == name)
}

#[derive(Debug, Clone, Serialize)]
pub struct JobStatus {
    pub job: &'static str,
    pub description: &'static str,
    pub every_seconds: u64,
    pub last_run: Option<JobRun>,
    pub due: bool,
}

pub async fn status(state: &AppState) -> anyhow::Result<Vec<JobStatus>> {
    let mut out = Vec::new();
    for j in registry(state) {
        let last = last_run(&state.pool, j.name).await?;
        let due = is_due(&j, last.as_ref(), Utc::now());
        out.push(JobStatus {
            job: j.name,
            description: j.description,
            every_seconds: j.every.as_secs(),
            last_run: last,
            due,
        });
    }
    Ok(out)
}

async fn last_run(pool: &PgPool, name: &str) -> sqlx::Result<Option<JobRun>> {
    sqlx::query_as::<_, JobRun>(
        "select * from job_runs where job = $1 and status <> 'skipped' order by started_at desc limit 1",
    )
    .bind(name)
    .fetch_optional(pool)
    .await
}

fn is_due(job: &Job, last: Option<&JobRun>, now: DateTime<Utc>) -> bool {
    match last {
        None => true,
        Some(r) if r.status == JobRunStatus::Running => {
            // A run that never finished (crash) blocks for at most an hour.
            now - r.started_at > chrono::Duration::hours(1)
        }
        Some(r) => {
            let since = now - r.finished_at.unwrap_or(r.started_at);
            since.num_seconds() as u64 >= job.every.as_secs()
        }
    }
}

/// Run one job now, under its advisory lock, recording a `job_runs` row.
/// Returns the finished row (status `skipped` if another instance held the lock).
pub async fn run_named(state: AppState, name: &str, triggered_by: &str) -> anyhow::Result<JobRun> {
    let job = get(&state, name).ok_or_else(|| anyhow::anyhow!("unknown job {name}"))?;
    let mut conn = state.pool.acquire().await?;
    let key = format!("hearth:job:{name}");
    let got: bool = sqlx::query_scalar("select pg_try_advisory_lock(hashtext($1))")
        .bind(&key)
        .fetch_one(&mut *conn)
        .await?;
    if !got {
        return Ok(sqlx::query_as::<_, JobRun>(
            "insert into job_runs (job, status, finished_at, triggered_by, summary) values ($1, 'skipped', now(), $2, '{\"reason\":\"locked elsewhere\"}') returning *",
        )
        .bind(name)
        .bind(triggered_by)
        .fetch_one(&state.pool)
        .await?);
    }
    let run = sqlx::query_as::<_, JobRun>(
        "insert into job_runs (job, status, triggered_by) values ($1, 'running', $2) returning *",
    )
    .bind(name)
    .bind(triggered_by)
    .fetch_one(&state.pool)
    .await?;
    let started = std::time::Instant::now();
    let result = (job.handler)(state.clone()).await;
    let elapsed_ms = started.elapsed().as_millis() as i64;
    let finished = match result {
        Ok(mut summary) => {
            if let Value::Object(m) = &mut summary {
                m.insert("elapsed_ms".into(), Value::from(elapsed_ms));
            }
            tracing::info!(job = name, elapsed_ms, "job ok");
            sqlx::query_as::<_, JobRun>(
                "update job_runs set status = 'ok', finished_at = now(), summary = $2 where id = $1 returning *",
            )
            .bind(run.id)
            .bind(summary)
            .fetch_one(&state.pool)
            .await?
        }
        Err(e) => {
            tracing::error!(job = name, error = %e, "job failed");
            sqlx::query_as::<_, JobRun>(
                "update job_runs set status = 'failed', finished_at = now(), error = $2 where id = $1 returning *",
            )
            .bind(run.id)
            .bind(e.to_string())
            .fetch_one(&state.pool)
            .await?
        }
    };
    let _ = sqlx::query("select pg_advisory_unlock(hashtext($1))")
        .bind(&key)
        .execute(&mut *conn)
        .await;
    Ok(finished)
}

/// The scheduler loop: every tick, run whatever is due, in registry order.
pub fn spawn_scheduler(state: AppState) {
    let tick = Duration::from_secs(state.config.jobs.tick_seconds.max(5));
    tokio::spawn(async move {
        // Let the server come up before the first pass.
        tokio::time::sleep(Duration::from_secs(3)).await;
        loop {
            for job in registry(&state) {
                let last = match last_run(&state.pool, job.name).await {
                    Ok(l) => l,
                    Err(e) => {
                        tracing::warn!(error = %e, "scheduler: cannot read job_runs");
                        None
                    }
                };
                if is_due(&job, last.as_ref(), Utc::now())
                    && let Err(e) = run_named(state.clone(), job.name, "schedule").await
                {
                    tracing::error!(job = job.name, error = %e, "scheduler: run failed");
                }
            }
            tokio::time::sleep(tick).await;
        }
    });
}
