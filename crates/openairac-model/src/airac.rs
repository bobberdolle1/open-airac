//! Authoritative ICAO 28-day AIRAC Cycle Calendar & Lifecycle Engine.
//!
//! Enforces exact 28-day AIRAC cycle arithmetic:
//! - Anchor Epoch: Cycle 2001 started on 2020-01-30.
//! - Each cycle lasts exactly 28 days (4 weeks).
//! - 13 cycles per nominal year.
//! - Distinguishes:
//!   * `current_effective_cycle`: The AIRAC cycle active at a given date.
//!   * `effective_from`: The official start date of the cycle (00:00:00 UTC).
//!   * `valid_through`: The official expiration date of the cycle (23:59:59 UTC).
//!   * `retrieved_at`: Wall-clock snapshot download timestamp (NEVER confused with cycle start).
//!   * `row_revision_cycle`: Dataset row-level revision provenance.

use anyhow::{Result, bail};
use chrono::{DateTime, Duration, NaiveDate, TimeZone, Utc};
use serde::{Deserialize, Serialize};

/// Reference epoch: ICAO Cycle 2001 (2020-01-30).
pub const AIRAC_EPOCH_DATE: (i32, u32, u32) = (2020, 1, 30);

/// Length of standard ICAO AIRAC cycle in days.
pub const AIRAC_CYCLE_DAYS: i64 = 28;

/// Metadata describing an AIRAC cycle's temporal boundaries.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AiracCycleInfo {
    /// 4-character cycle identifier (e.g. "2608", "2601", "2701").
    pub cycle: String,
    /// 4-digit full year (e.g. 2026).
    pub year: i32,
    /// 1-based cycle index within year (1..=13).
    pub cycle_number: u32,
    /// Effective start timestamp (00:00:00 UTC on cycle start day).
    pub effective_from: DateTime<Utc>,
    /// Expiration timestamp (23:59:59 UTC on day 28).
    pub valid_through: DateTime<Utc>,
    /// Next cycle code (e.g. "2609").
    pub next_cycle: String,
    /// Next cycle effective timestamp.
    pub next_effective_from: DateTime<Utc>,
    /// Previous cycle code (e.g. "2607").
    pub previous_cycle: String,
}

impl AiracCycleInfo {
    /// Compute the AIRAC cycle effective at a specific date/time.
    pub fn for_date(date: DateTime<Utc>) -> Self {
        let epoch =
            NaiveDate::from_ymd_opt(AIRAC_EPOCH_DATE.0, AIRAC_EPOCH_DATE.1, AIRAC_EPOCH_DATE.2)
                .expect("valid epoch date");

        let days = date.date_naive().signed_duration_since(epoch).num_days();
        let cycle_index = if days >= 0 {
            days / AIRAC_CYCLE_DAYS
        } else {
            (days - AIRAC_CYCLE_DAYS + 1) / AIRAC_CYCLE_DAYS
        };

        let year = 2020 + (cycle_index / 13) as i32;
        let mut num = (cycle_index % 13) as i32 + 1;
        let mut actual_year = year;
        if num <= 0 {
            num += 13;
            actual_year -= 1;
        }
        let start_date = epoch + Duration::days(cycle_index * AIRAC_CYCLE_DAYS);
        let end_date = start_date + Duration::days(AIRAC_CYCLE_DAYS - 1);

        let cycle_code = format!("{:02}{:02}", actual_year % 100, num);

        // Next and previous cycle codes
        let next_info = Self::for_cycle_index(cycle_index + 1, epoch);
        let prev_info = Self::for_cycle_index(cycle_index - 1, epoch);

        Self {
            cycle: cycle_code,
            year: actual_year,
            cycle_number: num as u32,
            effective_from: Utc.from_utc_datetime(&start_date.and_hms_opt(0, 0, 0).unwrap()),
            valid_through: Utc.from_utc_datetime(&end_date.and_hms_opt(23, 59, 59).unwrap()),
            next_cycle: next_info.0,
            next_effective_from: next_info.1,
            previous_cycle: prev_info.0,
        }
    }

    /// Compute cycle boundaries from an explicit 4-digit cycle code (e.g. "2608" or "202608").
    pub fn parse_cycle(cycle_str: &str) -> Result<Self> {
        let clean = cycle_str.trim();
        let (yr_short, num_str) = if clean.len() == 4 {
            (&clean[..2], &clean[2..])
        } else if clean.len() == 6 {
            (&clean[2..4], &clean[4..])
        } else {
            bail!(
                "Invalid AIRAC cycle string format: '{}' (expected YYNN or YYYYNN)",
                clean
            );
        };

        let yr_val: i32 = yr_short.parse()?;
        let full_year = if yr_val >= 70 {
            1900 + yr_val
        } else {
            2000 + yr_val
        };
        let num_val: u32 = num_str.parse()?;

        if !(1..=14).contains(&num_val) {
            bail!("Invalid cycle number {} (must be 1..=13)", num_val);
        }

        let epoch =
            NaiveDate::from_ymd_opt(AIRAC_EPOCH_DATE.0, AIRAC_EPOCH_DATE.1, AIRAC_EPOCH_DATE.2)
                .expect("valid epoch date");

        let years_diff = full_year - 2020;
        let cycle_index = (years_diff * 13) + (num_val as i32 - 1);
        let start_date = epoch + Duration::days(cycle_index as i64 * AIRAC_CYCLE_DAYS);
        let start_dt = Utc.from_utc_datetime(&start_date.and_hms_opt(0, 0, 0).unwrap());

        Ok(Self::for_date(start_dt))
    }

    fn for_cycle_index(idx: i64, epoch: NaiveDate) -> (String, DateTime<Utc>) {
        let year = 2020 + (idx / 13) as i32;
        let mut num = (idx % 13) as i32 + 1;
        let mut actual_year = year;
        if num <= 0 {
            num += 13;
            actual_year -= 1;
        }
        let start_date = epoch + Duration::days(idx * AIRAC_CYCLE_DAYS);
        let code = format!("{:02}{:02}", actual_year % 100, num);
        let dt = Utc.from_utc_datetime(&start_date.and_hms_opt(0, 0, 0).unwrap());
        (code, dt)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_airac_2608_authoritative_dates() {
        // AIRAC 2608 must start on 2026-08-06
        let info = AiracCycleInfo::parse_cycle("2608").expect("parse 2608");
        assert_eq!(info.cycle, "2608");
        assert_eq!(info.year, 2026);
        assert_eq!(info.cycle_number, 8);
        assert_eq!(
            info.effective_from.date_naive(),
            NaiveDate::from_ymd_opt(2026, 8, 6).unwrap()
        );
        assert_eq!(
            info.valid_through.date_naive(),
            NaiveDate::from_ymd_opt(2026, 9, 2).unwrap()
        );
        assert_eq!(info.next_cycle, "2609");
        assert_eq!(
            info.next_effective_from.date_naive(),
            NaiveDate::from_ymd_opt(2026, 9, 3).unwrap()
        );

        // For date 2026-08-20 (today), active cycle must be 2608
        let today = Utc.from_utc_datetime(
            &NaiveDate::from_ymd_opt(2026, 8, 20)
                .unwrap()
                .and_hms_opt(12, 0, 0)
                .unwrap(),
        );
        let current = AiracCycleInfo::for_date(today);
        assert_eq!(current.cycle, "2608");
        assert_eq!(
            current.effective_from.date_naive(),
            NaiveDate::from_ymd_opt(2026, 8, 6).unwrap()
        );
    }

    #[test]
    fn test_airac_test_vectors_2026() {
        let test_vectors = [
            ("2601", 2026, 1, 2026, 1, 22, 2026, 2, 18),
            ("2607", 2026, 7, 2026, 7, 9, 2026, 8, 5),
            ("2608", 2026, 8, 2026, 8, 6, 2026, 9, 2),
            ("2609", 2026, 9, 2026, 9, 3, 2026, 9, 30),
            ("2610", 2026, 10, 2026, 10, 1, 2026, 10, 28),
            ("2613", 2026, 13, 2026, 12, 24, 2027, 1, 20),
            ("2701", 2027, 1, 2027, 1, 21, 2027, 2, 17),
        ];

        for (code, exp_yr, exp_num, s_y, s_m, s_d, e_y, e_m, e_d) in test_vectors {
            let info = AiracCycleInfo::parse_cycle(code).unwrap();
            assert_eq!(info.cycle, code);
            assert_eq!(info.year, exp_yr);
            assert_eq!(info.cycle_number, exp_num);
            assert_eq!(
                info.effective_from.date_naive(),
                NaiveDate::from_ymd_opt(s_y, s_m, s_d).unwrap(),
                "Effective from mismatch for cycle {}",
                code
            );
            assert_eq!(
                info.valid_through.date_naive(),
                NaiveDate::from_ymd_opt(e_y, e_m, e_d).unwrap(),
                "Valid through mismatch for cycle {}",
                code
            );
        }
    }
}
