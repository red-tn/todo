// recur.rs — repeating tasks: how a due date advances, and what the next
// instance looks like.
//
// Completing a repeating task does not reopen it. It leaves the finished one in
// Done, preserving history, and creates a successor with the date moved on.
use crate::model::Todo;
use chrono::{Datelike, NaiveDate};

/// How often a task repeats. Stored as a lowercase string so the column stays
/// readable in the database.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Unit {
    Daily,
    Weekly,
    Monthly,
    Yearly,
}

impl Unit {
    pub fn parse(s: &str) -> Option<Unit> {
        match s.trim().to_ascii_lowercase().as_str() {
            "daily" => Some(Unit::Daily),
            "weekly" => Some(Unit::Weekly),
            "monthly" => Some(Unit::Monthly),
            "yearly" => Some(Unit::Yearly),
            _ => None,
        }
    }
}

/// Last day of the given month, accounting for leap years.
fn days_in_month(year: i32, month: u32) -> u32 {
    let (ny, nm) = if month == 12 { (year + 1, 1) } else { (year, month + 1) };
    NaiveDate::from_ymd_opt(ny, nm, 1)
        .and_then(|d| d.pred_opt())
        .map(|d| d.day())
        .unwrap_or(28)
}

/// Move `date` forward by `interval` units.
///
/// Month and year steps clamp to the last valid day of the destination month,
/// so 31 January plus one month is 28 February rather than an error or a slide
/// into March. This is what calendar apps do, and the alternative — skipping
/// months without a 31st — silently drops occurrences.
pub fn advance(date: NaiveDate, unit: Unit, interval: i64) -> NaiveDate {
    let n = interval.max(1);
    match unit {
        Unit::Daily => date + chrono::Duration::days(n),
        Unit::Weekly => date + chrono::Duration::weeks(n),
        Unit::Monthly => add_months(date, n),
        Unit::Yearly => add_months(date, n * 12),
    }
}

fn add_months(date: NaiveDate, months: i64) -> NaiveDate {
    let total = date.year() as i64 * 12 + (date.month() as i64 - 1) + months;
    let year = (total.div_euclid(12)) as i32;
    let month = (total.rem_euclid(12) + 1) as u32;
    let day = date.day().min(days_in_month(year, month));
    NaiveDate::from_ymd_opt(year, month, day).unwrap_or(date)
}

/// Build the successor to a task that was just completed.
///
/// Returns `None` when the task does not repeat, so the caller can treat
/// "no recurrence" and "nothing to do" identically.
///
/// `today` is the base when the task has no due date — a repeating task without
/// a date still needs somewhere to start counting from.
pub fn next_instance(done: &Todo, today: NaiveDate, new_id: String) -> Option<Todo> {
    let unit = Unit::parse(done.recurrence.as_deref()?)?;

    let base = done
        .due
        .as_ref()
        .and_then(|d| NaiveDate::parse_from_str(d, "%Y-%m-%d").ok())
        .unwrap_or(today);

    let mut next_due = advance(base, unit, done.recurrence_interval as i64);

    // A task completed long after it was due would otherwise spawn a successor
    // that is itself already overdue. Keep stepping until it is in the future.
    let mut guard = 0;
    while next_due <= today && guard < 1000 {
        next_due = advance(next_due, unit, done.recurrence_interval as i64);
        guard += 1;
    }

    Some(Todo {
        id: new_id,
        title: done.title.clone(),
        note: done.note.clone(),
        due: Some(next_due.format("%Y-%m-%d").to_string()),
        priority: done.priority.clone(),
        done: false,
        tags: done.tags.clone(),
        created_at: chrono::Utc::now(),
        updated_at: Some(chrono::Utc::now()),
        deleted_at: None,
        archived_at: None,
        dirty: true,
        recurrence: done.recurrence.clone(),
        recurrence_interval: done.recurrence_interval,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn day(s: &str) -> NaiveDate {
        NaiveDate::parse_from_str(s, "%Y-%m-%d").unwrap()
    }

    fn repeating(due: Option<&str>, unit: &str, interval: i32) -> Todo {
        Todo {
            id: "a".into(),
            title: "Pay the bills".into(),
            note: "every month".into(),
            due: due.map(|s| s.to_string()),
            priority: Some("high".into()),
            done: true,
            tags: vec!["expenses".into()],
            created_at: chrono::Utc::now(),
            updated_at: None,
            deleted_at: None,
            archived_at: None,
            dirty: false,
            recurrence: Some(unit.into()),
            recurrence_interval: interval,
        }
    }

    #[test]
    fn advances_by_days_and_weeks() {
        assert_eq!(advance(day("2026-08-04"), Unit::Daily, 1), day("2026-08-05"));
        assert_eq!(advance(day("2026-08-04"), Unit::Daily, 10), day("2026-08-14"));
        assert_eq!(advance(day("2026-08-04"), Unit::Weekly, 2), day("2026-08-18"));
    }

    #[test]
    fn month_end_clamps_instead_of_overflowing() {
        // The case that breaks naive implementations.
        assert_eq!(advance(day("2026-01-31"), Unit::Monthly, 1), day("2026-02-28"));
        assert_eq!(advance(day("2026-03-31"), Unit::Monthly, 1), day("2026-04-30"));
        assert_eq!(advance(day("2026-05-31"), Unit::Monthly, 3), day("2026-08-31"));
    }

    #[test]
    fn clamping_respects_leap_years() {
        assert_eq!(advance(day("2028-01-31"), Unit::Monthly, 1), day("2028-02-29"));
        assert_eq!(advance(day("2028-02-29"), Unit::Yearly, 1), day("2029-02-28"));
    }

    #[test]
    fn monthly_crosses_the_year_boundary() {
        assert_eq!(advance(day("2026-11-15"), Unit::Monthly, 3), day("2027-02-15"));
        assert_eq!(advance(day("2026-12-01"), Unit::Monthly, 1), day("2027-01-01"));
    }

    #[test]
    fn yearly_advances_whole_years() {
        assert_eq!(advance(day("2026-08-04"), Unit::Yearly, 1), day("2027-08-04"));
        assert_eq!(advance(day("2026-08-04"), Unit::Yearly, 2), day("2028-08-04"));
    }

    #[test]
    fn an_interval_below_one_is_treated_as_one() {
        // Guards against a zero from the UI producing an infinite loop.
        assert_eq!(advance(day("2026-08-04"), Unit::Daily, 0), day("2026-08-05"));
    }

    #[test]
    fn non_repeating_tasks_have_no_successor() {
        let mut t = repeating(Some("2026-08-04"), "monthly", 1);
        t.recurrence = None;
        assert!(next_instance(&t, day("2026-08-04"), "new".into()).is_none());
    }

    #[test]
    fn unrecognised_recurrence_produces_no_successor() {
        let mut t = repeating(Some("2026-08-04"), "monthly", 1);
        t.recurrence = Some("fortnightly".into());
        assert!(next_instance(&t, day("2026-08-04"), "new".into()).is_none());
    }

    #[test]
    fn successor_carries_the_task_forward_but_starts_fresh() {
        let t = repeating(Some("2026-08-04"), "monthly", 1);
        let next = next_instance(&t, day("2026-08-04"), "new-id".into()).unwrap();

        assert_eq!(next.id, "new-id", "must be a new task, not the same one");
        assert_eq!(next.title, "Pay the bills");
        assert_eq!(next.note, "every month");
        assert_eq!(next.tags, vec!["expenses"]);
        assert_eq!(next.priority.as_deref(), Some("high"));
        assert_eq!(next.due.as_deref(), Some("2026-09-04"));
        assert!(!next.done, "the successor starts open");
        assert!(next.dirty, "must be queued for push");
        assert_eq!(next.recurrence.as_deref(), Some("monthly"));
        assert!(!next.is_archived(), "a fresh occurrence is not archived");
    }

    #[test]
    fn a_late_completion_lands_on_the_next_future_occurrence() {
        // Due 15 June, ticked off on 4 August. Stepping monthly gives 15 July
        // (already past, so skipped) then 15 August — the next date that has
        // not happened yet. The series stays on the 15th rather than drifting
        // to the completion date, and no future occurrence is skipped.
        let t = repeating(Some("2026-06-15"), "monthly", 1);
        let next = next_instance(&t, day("2026-08-04"), "new".into()).unwrap();
        assert_eq!(next.due.as_deref(), Some("2026-08-15"));
    }

    #[test]
    fn a_very_late_completion_does_not_produce_an_overdue_successor() {
        // Nearly a year late: the successor must still be in the future.
        let t = repeating(Some("2025-09-15"), "monthly", 1);
        let next = next_instance(&t, day("2026-08-04"), "new".into()).unwrap();
        assert_eq!(next.due.as_deref(), Some("2026-08-15"));
    }

    #[test]
    fn a_repeating_task_with_no_due_date_counts_from_today() {
        let t = repeating(None, "weekly", 1);
        let next = next_instance(&t, day("2026-08-04"), "new".into()).unwrap();
        assert_eq!(next.due.as_deref(), Some("2026-08-11"));
    }
}
