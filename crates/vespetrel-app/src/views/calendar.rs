//! Calendar Grid Views & PIM UI §6 & §7 Phase 3
use chrono::{DateTime, Datelike, Duration, NaiveDate, Utc};
use vespetrel_core::CalendarEvent;
use vespetrel_dav::rrule::RRule;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CalendarViewMode {
    Month,
    Week,
    Day,
    Agenda,
}

#[derive(Debug, Clone)]
pub struct CalendarView {
    pub mode: CalendarViewMode,
    pub selected_date: NaiveDate,
    pub events: Vec<CalendarEvent>,
    pub selected_calendar_id: Option<String>,
}

impl CalendarView {
    pub fn new() -> Self {
        let today = Utc::now().date_naive();
        Self {
            mode: CalendarViewMode::Month,
            selected_date: today,
            events: Vec::new(),
            selected_calendar_id: None,
        }
    }

    pub fn set_mode(&mut self, mode: CalendarViewMode) {
        self.mode = mode;
    }

    pub fn set_date(&mut self, date: NaiveDate) {
        self.selected_date = date;
    }

    pub fn add_event(&mut self, event: CalendarEvent) {
        self.events.push(event);
    }

    pub fn events_for_date(&self, date: NaiveDate) -> Vec<&CalendarEvent> {
        let day_start = DateTime::from_naive_utc_and_offset(
            date.and_hms_opt(0, 0, 0).expect("valid midnight"),
            Utc,
        );
        let day_end = DateTime::from_naive_utc_and_offset(
            date.and_hms_opt(23, 59, 59).expect("valid end of day"),
            Utc,
        );
        self.events_in_range(day_start, day_end)
    }

    pub fn events_in_range(&self, start: DateTime<Utc>, end: DateTime<Utc>) -> Vec<&CalendarEvent> {
        self.events
            .iter()
            .filter(|e| {
                if e.end >= start && e.start <= end {
                    return true;
                }
                if let Some(raw) = &e.raw_ical
                    && let Some(rrule_line) = raw
                        .lines()
                        .find(|l| l.trim_start().to_ascii_uppercase().starts_with("RRULE:"))
                    && let Some(rule) = RRule::parse(rrule_line)
                {
                    let dur = e.end.signed_duration_since(e.start);
                    let search_start = start - dur;
                    let occ = rule.expand(e.start, search_start, end, 250);
                    return occ.into_iter().any(|occ_start| {
                        let occ_end = occ_start + dur;
                        occ_end >= start && occ_start <= end
                    });
                }
                false
            })
            .collect()
    }

    /// Generates days for the month grid view (including preceding/trailing days for a 7x6 grid)
    pub fn month_grid_days(&self) -> Vec<NaiveDate> {
        let first_day =
            NaiveDate::from_ymd_opt(self.selected_date.year(), self.selected_date.month(), 1)
                .unwrap_or(self.selected_date);
        let weekday = first_day.weekday().num_days_from_monday(); // 0 = Mon, 6 = Sun
        let grid_start = first_day - Duration::days(weekday as i64);

        let mut days = Vec::with_capacity(42);
        for i in 0..42 {
            days.push(grid_start + Duration::days(i));
        }
        days
    }
}

impl Default for CalendarView {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_calendar_grid_and_events() {
        let mut view = CalendarView::new();
        let test_date = NaiveDate::from_ymd_opt(2026, 8, 15).unwrap();
        view.set_date(test_date);

        let start =
            DateTime::from_naive_utc_and_offset(test_date.and_hms_opt(10, 0, 0).unwrap(), Utc);
        let end =
            DateTime::from_naive_utc_and_offset(test_date.and_hms_opt(11, 0, 0).unwrap(), Utc);

        let event = CalendarEvent {
            id: "ev1".into(),
            calendar_id: "cal1".into(),
            title: "Team Standup".into(),
            description: Some("Daily sync".into()),
            start,
            end,
            location: Some("Room A".into()),
            ical_uid: Some("uid-1".into()),
            raw_ical: None,
        };
        view.add_event(event);

        let evs = view.events_for_date(test_date);
        assert_eq!(evs.len(), 1);
        assert_eq!(evs[0].title, "Team Standup");

        let grid = view.month_grid_days();
        assert_eq!(grid.len(), 42);
    }

    #[test]
    fn test_recurring_rrule_event_appears_on_future_dates() {
        let mut view = CalendarView::new();
        let day1 = NaiveDate::from_ymd_opt(2026, 8, 10).unwrap();
        let day3 = NaiveDate::from_ymd_opt(2026, 8, 12).unwrap();
        let start = DateTime::from_naive_utc_and_offset(day1.and_hms_opt(9, 0, 0).unwrap(), Utc);
        let end = DateTime::from_naive_utc_and_offset(day1.and_hms_opt(10, 0, 0).unwrap(), Utc);
        view.add_event(CalendarEvent {
            id: "rec1".into(),
            calendar_id: "cal1".into(),
            title: "Daily Standup".into(),
            description: None,
            start,
            end,
            location: None,
            ical_uid: None,
            raw_ical: Some("BEGIN:VEVENT\nRRULE:FREQ=DAILY;COUNT=5\nEND:VEVENT".into()),
        });
        assert_eq!(view.events_for_date(day3).len(), 1);
    }
}
