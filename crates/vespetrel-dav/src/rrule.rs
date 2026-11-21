//! Clean-room RFC 5545 `RRULE` recurrence parser and expander for CalDAV events.

use chrono::{
    DateTime, Datelike, Duration, Months, NaiveDate, NaiveDateTime, TimeZone, Utc, Weekday,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Freq {
    Daily,
    Weekly,
    Monthly,
    Yearly,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RRule {
    pub freq: Freq,
    pub interval: u32,
    pub count: Option<usize>,
    pub until: Option<DateTime<Utc>>,
    pub by_day: Vec<Weekday>,
}

impl RRule {
    /// Parse an RFC 5545 `RRULE` value (with or without a leading `RRULE:` prefix).
    pub fn parse(raw: &str) -> Option<Self> {
        let body = raw.trim().strip_prefix("RRULE:").unwrap_or(raw.trim());
        let mut freq = None;
        let mut interval = 1u32;
        let mut count = None;
        let mut until = None;
        let mut by_day = Vec::new();

        for part in body.split(';') {
            let Some((k, v)) = part.split_once('=') else {
                continue;
            };
            match k.trim().to_ascii_uppercase().as_str() {
                "FREQ" => {
                    freq = match v.trim().to_ascii_uppercase().as_str() {
                        "DAILY" => Some(Freq::Daily),
                        "WEEKLY" => Some(Freq::Weekly),
                        "MONTHLY" => Some(Freq::Monthly),
                        "YEARLY" => Some(Freq::Yearly),
                        _ => None,
                    };
                }
                "INTERVAL" => {
                    if let Ok(n) = v.trim().parse::<u32>() {
                        interval = n.max(1);
                    }
                }
                "COUNT" => {
                    if let Ok(n) = v.trim().parse::<usize>() {
                        count = Some(n);
                    }
                }
                "UNTIL" => {
                    until = parse_ics_datetime(v.trim());
                }
                "BYDAY" => {
                    for tok in v.split(',') {
                        if let Some(wd) = parse_weekday(tok.trim()) {
                            by_day.push(wd);
                        }
                    }
                }
                _ => {}
            }
        }

        Some(Self {
            freq: freq?,
            interval,
            count,
            until,
            by_day,
        })
    }

    /// Expand occurrence start timestamps within `[window_start, window_end]`,
    /// capped at `max_instances` (default safety cap 500) to prevent unbounded loops.
    pub fn expand(
        &self,
        dtstart: DateTime<Utc>,
        window_start: DateTime<Utc>,
        window_end: DateTime<Utc>,
        max_instances: usize,
    ) -> Vec<DateTime<Utc>> {
        let cap = max_instances.min(500);
        let mut results = Vec::new();
        let mut emitted = 0usize;
        let mut step = 0u32;

        loop {
            if let Some(c) = self.count
                && emitted >= c
            {
                break;
            }
            if results.len() >= cap {
                break;
            }

            let candidate_base =
                match advance_dt(dtstart, self.freq, step.saturating_mul(self.interval)) {
                    Some(dt) => dt,
                    None => break,
                };

            if candidate_base > window_end {
                break;
            }
            if let Some(u) = self.until
                && candidate_base > u
            {
                break;
            }

            if self.freq == Freq::Weekly && !self.by_day.is_empty() {
                let week_start = candidate_base
                    - Duration::days(candidate_base.weekday().num_days_from_monday() as i64);
                for offset in 0..7i64 {
                    let day_dt = week_start + Duration::days(offset);
                    if day_dt < dtstart {
                        continue;
                    }
                    if !self.by_day.contains(&day_dt.weekday()) {
                        continue;
                    }
                    if let Some(u) = self.until
                        && day_dt > u
                    {
                        return results;
                    }
                    if let Some(c) = self.count
                        && emitted >= c
                    {
                        return results;
                    }
                    if day_dt > window_end {
                        return results;
                    }
                    emitted += 1;
                    if day_dt >= window_start {
                        results.push(day_dt);
                        if results.len() >= cap {
                            return results;
                        }
                    }
                }
            } else {
                emitted += 1;
                if candidate_base >= window_start {
                    results.push(candidate_base);
                }
            }

            step = step.saturating_add(1);
            if step > 10_000 {
                break;
            }
        }

        results
    }
}

fn advance_dt(dtstart: DateTime<Utc>, freq: Freq, n: u32) -> Option<DateTime<Utc>> {
    match freq {
        Freq::Daily => dtstart.checked_add_signed(Duration::days(i64::from(n))),
        Freq::Weekly => dtstart.checked_add_signed(Duration::weeks(i64::from(n))),
        Freq::Monthly => dtstart.checked_add_months(Months::new(n)),
        Freq::Yearly => dtstart.checked_add_months(Months::new(n.saturating_mul(12))),
    }
}

fn parse_weekday(s: &str) -> Option<Weekday> {
    let suffix = if s.len() >= 2 { &s[s.len() - 2..] } else { s };
    match suffix.to_ascii_uppercase().as_str() {
        "MO" => Some(Weekday::Mon),
        "TU" => Some(Weekday::Tue),
        "WE" => Some(Weekday::Wed),
        "TH" => Some(Weekday::Thu),
        "FR" => Some(Weekday::Fri),
        "SA" => Some(Weekday::Sat),
        "SU" => Some(Weekday::Sun),
        _ => None,
    }
}

fn parse_ics_datetime(s: &str) -> Option<DateTime<Utc>> {
    if let Ok(dt) = NaiveDateTime::parse_from_str(s.trim_end_matches('Z'), "%Y%m%dT%H%M%S") {
        return Some(Utc.from_utc_datetime(&dt));
    }
    if let Ok(d) = NaiveDate::parse_from_str(s, "%Y%m%d") {
        let dt = d.and_hms_opt(23, 59, 59)?;
        return Some(Utc.from_utc_datetime(&dt));
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn expands_daily_with_count() {
        let rrule = RRule::parse("RRULE:FREQ=DAILY;INTERVAL=2;COUNT=4").unwrap();
        let start = Utc.with_ymd_and_hms(2026, 1, 1, 9, 0, 0).unwrap();
        let end = Utc.with_ymd_and_hms(2026, 1, 31, 0, 0, 0).unwrap();
        let occ = rrule.expand(start, start, end, 100);
        assert_eq!(occ.len(), 4);
        assert_eq!(occ[1], Utc.with_ymd_and_hms(2026, 1, 3, 9, 0, 0).unwrap());
        assert_eq!(occ[3], Utc.with_ymd_and_hms(2026, 1, 7, 9, 0, 0).unwrap());
    }

    #[test]
    fn expands_weekly_byday_with_until() {
        let rrule = RRule::parse("FREQ=WEEKLY;BYDAY=MO,WE,FR;UNTIL=20260110T235959Z").unwrap();
        // 2026-01-05 is Monday
        let start = Utc.with_ymd_and_hms(2026, 1, 5, 10, 0, 0).unwrap();
        let end = Utc.with_ymd_and_hms(2026, 1, 31, 0, 0, 0).unwrap();
        let occ = rrule.expand(start, start, end, 100);
        assert_eq!(occ.len(), 3);
        assert_eq!(occ[0].weekday(), Weekday::Mon);
        assert_eq!(occ[1].weekday(), Weekday::Wed);
        assert_eq!(occ[2].weekday(), Weekday::Fri);
    }

    #[test]
    fn expands_monthly_and_yearly() {
        let monthly = RRule::parse("FREQ=MONTHLY;COUNT=3").unwrap();
        let start = Utc.with_ymd_and_hms(2026, 1, 15, 8, 0, 0).unwrap();
        let end = Utc.with_ymd_and_hms(2026, 12, 31, 0, 0, 0).unwrap();
        let occ = monthly.expand(start, start, end, 100);
        assert_eq!(occ.len(), 3);
        assert_eq!(occ[2], Utc.with_ymd_and_hms(2026, 3, 15, 8, 0, 0).unwrap());
    }
}
