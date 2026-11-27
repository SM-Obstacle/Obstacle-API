//! A small cron scheduler for the SOCC jobs.
//!
//! Each job implements the [`Job`] trait, and is registered with a cron [`Schedule`] in a named
//! group of the [`Scheduler`]:
//!
//! * Jobs in different groups run concurrently, each group in its own task.
//! * Jobs in the same group never overlap. When several of them are due at the same time, they
//!   run in the order they were registered in.
//!
//! On startup, every job that asks for it (see [`Job::run_on_startup`]) runs once, in registration
//! order, before the regular schedule kicks in.
//!
//! A failing run is logged, and the job waits for its next occurrence: it doesn't stop the other
//! jobs, nor the scheduler.

use std::{future::Future, pin::Pin, time::Duration};

use anyhow::Context as _;
use chrono::{DateTime, TimeDelta, Utc};
use cron::Schedule;
use futures::future::try_join_all;
use records_lib::Database;
use tokio::time;

/// The maximum time the scheduler sleeps before checking the wall clock again.
///
/// The tokio timers use a monotonic clock, so a long sleep could drift from the wall clock that the
/// cron schedules are based on.
const MAX_SLEEP: Duration = Duration::from_secs(60);

/// The information given to a job when it runs.
#[derive(Clone, Copy)]
pub struct JobContext<'a> {
    /// The database connections.
    pub db: &'a Database,
    /// The duration between the last two occurrences of the job's schedule, up to the time the
    /// run was planned at (the startup time for a startup run).
    ///
    /// For a regular run, this is the time elapsed since the previous occurrence. It is `None` if
    /// the schedule has less than two past occurrences.
    pub period: Option<TimeDelta>,
}

/// A job run periodically by the [`Scheduler`].
pub trait Job: Send + Sync + 'static {
    /// The name of the job, used in logs.
    fn name(&self) -> &'static str;

    /// Prepares the job, before any of the jobs runs.
    fn init(&self, _db: &Database) -> impl Future<Output = anyhow::Result<()>> + Send {
        async { Ok(()) }
    }

    /// Whether the job runs once on startup, before following its schedule.
    fn run_on_startup(&self) -> bool {
        true
    }

    /// Runs the job.
    fn run(&self, ctx: JobContext<'_>) -> impl Future<Output = anyhow::Result<()>> + Send;
}

type BoxFuture<'a, T> = Pin<Box<dyn Future<Output = T> + Send + 'a>>;

/// The object-safe version of [`Job`], to store jobs of different types together.
trait DynJob: Send + Sync {
    fn name(&self) -> &'static str;
    fn init<'a>(&'a self, db: &'a Database) -> BoxFuture<'a, anyhow::Result<()>>;
    fn run_on_startup(&self) -> bool;
    fn run<'a>(&'a self, ctx: JobContext<'a>) -> BoxFuture<'a, anyhow::Result<()>>;
}

impl<J: Job> DynJob for J {
    fn name(&self) -> &'static str {
        Job::name(self)
    }

    fn init<'a>(&'a self, db: &'a Database) -> BoxFuture<'a, anyhow::Result<()>> {
        Box::pin(Job::init(self, db))
    }

    fn run_on_startup(&self) -> bool {
        Job::run_on_startup(self)
    }

    fn run<'a>(&'a self, ctx: JobContext<'a>) -> BoxFuture<'a, anyhow::Result<()>> {
        Box::pin(Job::run(self, ctx))
    }
}

struct ScheduledJob {
    job: Box<dyn DynJob>,
    schedule: Schedule,
    next: Option<DateTime<Utc>>,
}

impl ScheduledJob {
    /// Plans the next run of the job, strictly after the provided time.
    fn plan_after(&mut self, after: DateTime<Utc>) {
        self.next = self.schedule.after(&after).next();
    }

    async fn run(&self, db: &Database, fire_time: DateTime<Utc>) {
        let name = self.job.name();
        let ctx = JobContext {
            db,
            period: period_until(&self.schedule, fire_time),
        };

        tracing::info!("Running job {name} (planned at {fire_time})");
        match self.job.run(ctx).await {
            Ok(()) => tracing::info!("Job {name} completed successfully"),
            Err(e) => tracing::error!("Job {name} returned an error: {e:#}"),
        }
    }
}

/// Returns the duration between the last two occurrences of the schedule, up to `time` (included).
fn period_until(schedule: &Schedule, time: DateTime<Utc>) -> Option<TimeDelta> {
    // The occurrences are at the second precision, so this includes `time` itself.
    let mut past = schedule.after(&(time + TimeDelta::seconds(1))).rev();
    let last = past.next()?;
    let before = past.next()?;
    Some(last - before)
}

struct Group {
    name: &'static str,
    jobs: Vec<ScheduledJob>,
}

impl Group {
    async fn run(mut self, db: Database) {
        let now = Utc::now();
        for job in &mut self.jobs {
            job.plan_after(now);
        }

        loop {
            // The earliest planned job, the first registered one in case of a tie.
            let Some((idx, fire_time)) = self
                .jobs
                .iter()
                .enumerate()
                .filter_map(|(idx, job)| job.next.map(|next| (idx, next)))
                .min_by_key(|&(idx, next)| (next, idx))
            else {
                tracing::warn!("No more job to plan in group {}", self.name);
                return;
            };

            sleep_until(fire_time).await;

            let job = &mut self.jobs[idx];
            job.run(&db, fire_time).await;
            // If the run took longer than the period, the missed occurrences are skipped.
            job.plan_after(Ord::max(fire_time, Utc::now()));
        }
    }
}

async fn sleep_until(time: DateTime<Utc>) {
    loop {
        let Ok(remaining) = (time - Utc::now()).to_std() else {
            return;
        };
        if remaining.is_zero() {
            return;
        }
        time::sleep(remaining.min(MAX_SLEEP)).await;
    }
}

/// The scheduler of the SOCC jobs. See the [module documentation](self) for more information.
#[derive(Default)]
pub struct Scheduler {
    groups: Vec<Group>,
}

impl Scheduler {
    /// Registers a job in the provided group, to run with the provided schedule.
    pub fn add<J: Job>(&mut self, group: &'static str, schedule: Schedule, job: J) -> &mut Self {
        let job = ScheduledJob {
            job: Box::new(job),
            schedule,
            next: None,
        };

        match self.groups.iter_mut().find(|g| g.name == group) {
            Some(g) => g.jobs.push(job),
            None => self.groups.push(Group {
                name: group,
                jobs: vec![job],
            }),
        }

        self
    }

    fn jobs(&self) -> impl Iterator<Item = &ScheduledJob> {
        self.groups.iter().flat_map(|g| &g.jobs)
    }

    /// Initializes the jobs, runs the startup ones, then runs every job on its schedule.
    ///
    /// This only returns if a job fails to initialize, or if the jobs have no occurrence left.
    pub async fn run(self, db: Database) -> anyhow::Result<()> {
        for job in self.jobs() {
            job.job
                .init(&db)
                .await
                .with_context(|| format!("cannot initialize job {}", job.job.name()))?;
        }

        for job in self.jobs() {
            tracing::info!(
                "Job {} planned with schedule `{}`",
                job.job.name(),
                job.schedule.source()
            );
        }

        let startup = Utc::now();
        for job in self.jobs().filter(|job| job.job.run_on_startup()) {
            job.run(&db, startup).await;
        }

        let handles = self
            .groups
            .into_iter()
            .map(|group| tokio::spawn(group.run(db.clone())));
        try_join_all(handles)
            .await
            .context("a scheduler group panicked")?;

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use chrono::TimeZone as _;

    use super::*;

    #[test]
    fn period_of_weekly_schedule() {
        let schedule: Schedule = "0 0 0 * * Mon".parse().unwrap();

        // On an occurrence
        let monday = Utc.with_ymd_and_hms(2026, 11, 23, 0, 0, 0).unwrap();
        assert_eq!(period_until(&schedule, monday), Some(TimeDelta::weeks(1)));

        // Between two occurrences
        let friday = Utc.with_ymd_and_hms(2026, 11, 27, 13, 37, 0).unwrap();
        assert_eq!(period_until(&schedule, friday), Some(TimeDelta::weeks(1)));
    }

    #[test]
    fn period_of_monthly_schedule() {
        let schedule: Schedule = "0 0 0 1 * *".parse().unwrap();
        let time = Utc.with_ymd_and_hms(2026, 3, 1, 0, 0, 0).unwrap();
        assert_eq!(period_until(&schedule, time), Some(TimeDelta::days(28)));
    }
}
