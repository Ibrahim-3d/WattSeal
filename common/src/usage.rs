use std::{
    sync::{Mutex, OnceLock},
    time::{Duration, Instant},
};

use chrono::{Datelike, Duration as ChronoDuration, Local, TimeZone};
use rusqlite::{OptionalExtension, params};

use crate::{
    Database, DatabaseError, TotalData,
    database::HOUR_MS,
    types::{MICROJOULES_PER_JOULE, SECONDS_PER_HOUR},
};

const MILLIS_PER_SECOND: f64 = 1_000.0;
const SECONDS_PER_DAY: f64 = 86_400.0;
const DEFAULT_LOOKBACK_DAYS: i64 = 7;

/// Energy and cost-ready usage aggregates derived from the persisted total-power history.
///
/// Values are intentionally expressed in energy/time units only. Currency and carbon
/// conversions stay in the UI so the same summary can be reused by the tray, CLI, or
/// other front ends.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct UsageSummary {
    /// Latest estimated total system power from the live `total_data` stream.
    pub current_power_w: f64,
    /// Energy recorded since local midnight.
    pub today_energy_wh: f64,
    /// Time for which WattSeal actually recorded data since local midnight.
    pub today_monitored_seconds: f64,
    /// Energy recorded since the start of the current local calendar month.
    pub month_energy_wh: f64,
    /// Time for which WattSeal actually recorded data this month.
    pub month_monitored_seconds: f64,
    /// Recent energy per calendar day, including periods where the computer was off.
    pub average_daily_energy_wh: f64,
    /// Recent energy per monitored hour (equivalent to average active power in Wh/h).
    pub average_active_hour_energy_wh: f64,
    /// Forecast for the full current month: actual month-to-date plus recent daily rate
    /// for the remaining calendar time.
    pub projected_month_energy_wh: f64,
    /// Number of calendar-day equivalents used by the recent average (maximum 7 days).
    pub projection_basis_days: f64,
}

#[derive(Debug, Clone, Copy, Default)]
struct RangeUsage {
    energy_uj: f64,
    monitored_ms: f64,
}

impl RangeUsage {
    fn energy_wh(self) -> f64 {
        self.energy_uj / MICROJOULES_PER_JOULE / SECONDS_PER_HOUR
    }

    fn monitored_seconds(self) -> f64 {
        self.monitored_ms / MILLIS_PER_SECOND
    }
}

impl Database {
    /// Computes dashboard-level energy aggregates from the existing `total_data` table.
    ///
    /// The query accounts for records that partially overlap a calendar boundary. This
    /// matters because WattSeal compacts old one-second samples into one-hour records.
    /// No schema migration or duplicate energy store is required.
    pub fn get_usage_summary(&self) -> Result<UsageSummary, DatabaseError> {
        if !self.total_data_table_exists()? {
            return Ok(UsageSummary::default());
        }

        let now = Local::now();
        let now_ms = now.timestamp_millis();
        let today_start = Local
            .with_ymd_and_hms(now.year(), now.month(), now.day(), 0, 0, 0)
            .earliest()
            .ok_or_else(|| DatabaseError::TimeError("Unable to resolve local start of day".to_string()))?;
        let month_start = Local
            .with_ymd_and_hms(now.year(), now.month(), 1, 0, 0, 0)
            .earliest()
            .ok_or_else(|| DatabaseError::TimeError("Unable to resolve local start of month".to_string()))?;
        let (next_month_year, next_month) = if now.month() == 12 {
            (now.year() + 1, 1)
        } else {
            (now.year(), now.month() + 1)
        };
        let next_month_start = Local
            .with_ymd_and_hms(next_month_year, next_month, 1, 0, 0, 0)
            .earliest()
            .ok_or_else(|| DatabaseError::TimeError("Unable to resolve local start of next month".to_string()))?;

        let today = self.usage_between(today_start.timestamp_millis(), now_ms)?;
        let month = self.usage_between(month_start.timestamp_millis(), now_ms)?;

        let requested_lookback_ms = (now - ChronoDuration::days(DEFAULT_LOOKBACK_DAYS)).timestamp_millis();
        let first_record_ms = self.first_total_timestamp()?.unwrap_or(now_ms);
        let lookback_start_ms = requested_lookback_ms.max(first_record_ms).min(now_ms);
        let recent = self.usage_between(lookback_start_ms, now_ms)?;

        let lookback_seconds = ((now_ms - lookback_start_ms).max(0) as f64) / MILLIS_PER_SECOND;
        let basis_days = lookback_seconds / SECONDS_PER_DAY;
        let recent_energy_wh = recent.energy_wh();
        let average_daily_energy_wh = if basis_days > 0.0 {
            recent_energy_wh / basis_days
        } else {
            0.0
        };
        let recent_monitored_hours = recent.monitored_seconds() / SECONDS_PER_HOUR;
        let average_active_hour_energy_wh = if recent_monitored_hours > 0.0 {
            recent_energy_wh / recent_monitored_hours
        } else {
            0.0
        };

        let remaining_days = ((next_month_start.timestamp_millis() - now_ms).max(0) as f64)
            / MILLIS_PER_SECOND
            / SECONDS_PER_DAY;
        let projected_month_energy_wh =
            project_month_energy(month.energy_wh(), average_daily_energy_wh, remaining_days);

        Ok(UsageSummary {
            current_power_w: self.latest_total_power_w()?.unwrap_or(0.0),
            today_energy_wh: today.energy_wh(),
            today_monitored_seconds: today.monitored_seconds(),
            month_energy_wh: month.energy_wh(),
            month_monitored_seconds: month.monitored_seconds(),
            average_daily_energy_wh,
            average_active_hour_energy_wh,
            projected_month_energy_wh,
            projection_basis_days: basis_days.min(DEFAULT_LOOKBACK_DAYS as f64),
        })
    }

    fn total_data_table_exists(&self) -> Result<bool, DatabaseError> {
        Ok(self.conn.query_row(
            "SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE type = 'table' AND name = ?1)",
            params![TotalData::table_name_static()],
            |row| row.get(0),
        )?)
    }

    fn first_total_timestamp(&self) -> Result<Option<i64>, DatabaseError> {
        let value = self
            .conn
            .query_row("SELECT MIN(timestamp) FROM total_data", [], |row| row.get::<_, Option<i64>>(0))
            .optional()?
            .flatten();
        Ok(value)
    }

    fn latest_total_power_w(&self) -> Result<Option<f64>, DatabaseError> {
        let row = self
            .conn
            .query_row(
                "SELECT CAST(total_energy_uj AS REAL), duration_ms \
                 FROM total_data \
                 WHERE duration_ms < ?1 \
                 ORDER BY timestamp DESC \
                 LIMIT 1",
                params![HOUR_MS],
                |row| Ok((row.get::<_, f64>(0)?, row.get::<_, i64>(1)?)),
            )
            .optional()?;

        Ok(row.map(|(energy_uj, duration_ms)| {
            let seconds = (duration_ms.max(1) as f64) / MILLIS_PER_SECOND;
            (energy_uj / MICROJOULES_PER_JOULE) / seconds
        }))
    }

    fn usage_between(&self, start_ms: i64, end_ms: i64) -> Result<RangeUsage, DatabaseError> {
        if end_ms <= start_ms {
            return Ok(RangeUsage::default());
        }

        let mut stmt = self.conn.prepare(
            "SELECT \
                COALESCE(SUM( \
                    CAST(total_energy_uj AS REAL) * \
                    CAST(MAX(0, MIN(timestamp + duration_ms, ?2) - MAX(timestamp, ?1)) AS REAL) / \
                    MAX(duration_ms, 1) \
                ), 0.0) AS weighted_energy_uj, \
                COALESCE(SUM( \
                    MAX(0, MIN(timestamp + duration_ms, ?2) - MAX(timestamp, ?1)) \
                ), 0) AS monitored_ms \
             FROM total_data \
             WHERE timestamp < ?2 \
               AND timestamp + duration_ms > ?1",
        )?;

        let usage = stmt.query_row(params![start_ms, end_ms], |row| {
            Ok(RangeUsage {
                energy_uj: row.get(0)?,
                monitored_ms: row.get::<_, f64>(1)?,
            })
        })?;
        Ok(usage)
    }
}

/// Returns a short-lived cached usage summary for views that render frequently.
///
/// The collector continues writing at 1 Hz, but calendar aggregates do not need to run
/// a multi-row SQL query every frame. Callers choose the acceptable staleness window.
pub fn cached_usage_summary(max_age: Duration) -> UsageSummary {
    static CACHE: OnceLock<Mutex<Option<(Instant, UsageSummary)>>> = OnceLock::new();

    let cache = CACHE.get_or_init(|| Mutex::new(None));
    if let Ok(guard) = cache.lock()
        && let Some((updated_at, summary)) = *guard
        && updated_at.elapsed() <= max_age
    {
        return summary;
    }

    let summary = Database::open_without_migrations()
        .and_then(|db| db.get_usage_summary())
        .unwrap_or_default();

    if let Ok(mut guard) = cache.lock() {
        *guard = Some((Instant::now(), summary));
    }
    summary
}

fn project_month_energy(month_to_date_wh: f64, average_daily_wh: f64, remaining_days: f64) -> f64 {
    month_to_date_wh.max(0.0) + average_daily_wh.max(0.0) * remaining_days.max(0.0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn projection_adds_recent_rate_only_to_remaining_days() {
        assert_eq!(project_month_energy(10_000.0, 2_000.0, 5.0), 20_000.0);
    }

    #[test]
    fn projection_never_subtracts_energy_for_invalid_inputs() {
        assert_eq!(project_month_energy(1_000.0, -10.0, 4.0), 1_000.0);
        assert_eq!(project_month_energy(1_000.0, 10.0, -4.0), 1_000.0);
    }
}
