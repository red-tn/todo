// notify.rs — the once-a-day digest of what is due and what is late.
//
// Tasks carry a due *date* with no time, so there is no per-task moment to fire
// at. One notification a day fits the data and is much harder to start ignoring
// than a stream of individual alerts.
use crate::model::Todo;
use chrono::{Local, NaiveDate};

/// How often to check whether the digest is due. Cheap: it compares two
/// numbers and almost always returns immediately.
pub const TICK_SECONDS: u64 = 60;

/// What a digest would say.
#[derive(Debug, PartialEq)]
pub struct Digest {
    pub due_today: Vec<String>,
    pub overdue: Vec<String>,
}

impl Digest {
    pub fn is_empty(&self) -> bool {
        self.due_today.is_empty() && self.overdue.is_empty()
    }

    /// Headline, e.g. "3 due today · 2 overdue".
    pub fn title(&self) -> String {
        let mut parts = Vec::new();
        if !self.due_today.is_empty() {
            parts.push(format!("{} due today", self.due_today.len()));
        }
        if !self.overdue.is_empty() {
            parts.push(format!("{} overdue", self.overdue.len()));
        }
        parts.join(" · ")
    }

    /// The first few titles, so the notification is actionable without opening
    /// the app. Overdue first — those are the ones being ignored.
    pub fn body(&self) -> String {
        const MAX: usize = 4;
        let all: Vec<&String> = self.overdue.iter().chain(self.due_today.iter()).collect();
        let shown: Vec<String> = all.iter().take(MAX).map(|s| s.to_string()).collect();
        let mut body = shown.join("\n");
        if all.len() > MAX {
            body.push_str(&format!("\n…and {} more", all.len() - MAX));
        }
        body
    }
}

/// Split live, unfinished tasks into due-today and overdue.
///
/// Completed and deleted tasks are excluded: a digest that counts things
/// already done trains you to ignore it.
pub fn build_digest(todos: &[Todo], today: NaiveDate) -> Digest {
    let mut due_today = Vec::new();
    let mut overdue = Vec::new();

    for t in todos {
        if t.done || t.is_deleted() {
            continue;
        }
        let Some(due) = t.due.as_ref() else { continue };
        let Ok(date) = NaiveDate::parse_from_str(due, "%Y-%m-%d") else {
            continue;
        };
        if date < today {
            overdue.push(t.title.clone());
        } else if date == today {
            due_today.push(t.title.clone());
        }
    }

    Digest { due_today, overdue }
}

/// Build the Slack message: overdue tasks always, then tasks whose due date
/// falls in a selected bucket — "today" (due today), "tomorrow" (due
/// tomorrow), "week" (2–7 days out). Buckets are independent, so any
/// combination works; an empty selection reports overdue tasks only.
///
/// Returns `None` when there is nothing to say — no message beats an empty one.
pub fn build_slack_message(todos: &[Todo], today: NaiveDate, buckets: &[String]) -> Option<String> {
    let has = |b: &str| buckets.iter().any(|s| s == b);
    let mut overdue: Vec<(NaiveDate, &str)> = Vec::new();
    let mut upcoming: Vec<(NaiveDate, &str)> = Vec::new();

    for t in todos {
        if t.done || t.is_deleted() || t.archived_at.is_some() {
            continue;
        }
        let Some(due) = t.due.as_ref() else { continue };
        let Ok(date) = NaiveDate::parse_from_str(due, "%Y-%m-%d") else {
            continue;
        };
        let days = (date - today).num_days();
        let wanted = match days {
            i64::MIN..=-1 => {
                overdue.push((date, &t.title));
                continue;
            }
            0 => has("today"),
            1 => has("tomorrow"),
            2..=7 => has("week"),
            _ => false,
        };
        if wanted {
            upcoming.push((date, &t.title));
        }
    }
    if overdue.is_empty() && upcoming.is_empty() {
        return None;
    }
    overdue.sort();
    upcoming.sort();

    let mut parts = Vec::new();
    if !overdue.is_empty() {
        parts.push(format!("{} overdue", overdue.len()));
    }
    if !upcoming.is_empty() {
        parts.push(format!("{} due soon", upcoming.len()));
    }
    let mut lines = vec![format!("To Do: {}", parts.join(" · "))];

    for (date, title) in &overdue {
        lines.push(format!("• {title} — overdue since {date}"));
    }
    for (date, title) in &upcoming {
        let label = match (*date - today).num_days() {
            0 => "due today".to_string(),
            1 => "due tomorrow".to_string(),
            _ => format!("due {}", date.format("%a %b %-d")),
        };
        lines.push(format!("• {title} — {label}"));
    }
    Some(lines.join("\n"))
}

/// Whether the digest should fire now.
///
/// Firing late is deliberate: if the app was not running at the configured
/// time, notifying on the next launch that same day beats staying silent.
pub fn should_fire(
    now: chrono::DateTime<Local>,
    hour: u32,
    minute: u32,
    last_fired: Option<NaiveDate>,
) -> bool {
    use chrono::Timelike;
    let today = now.date_naive();
    if last_fired == Some(today) {
        return false;
    }
    let mins_now = now.hour() * 60 + now.minute();
    mins_now >= hour * 60 + minute
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone;

    fn todo(title: &str, due: Option<&str>, done: bool) -> Todo {
        Todo {
            id: title.into(),
            title: title.into(),
            note: String::new(),
            due: due.map(|s| s.to_string()),
            priority: None,
            done,
            tags: vec![],
            created_at: chrono::Utc::now(),
            updated_at: None,
            deleted_at: None,
            archived_at: None,
            dirty: false,
            recurrence: None,
            recurrence_interval: 1,
        }
    }

    fn day(s: &str) -> NaiveDate {
        NaiveDate::parse_from_str(s, "%Y-%m-%d").unwrap()
    }

    #[test]
    fn splits_due_today_from_overdue() {
        let todos = vec![
            todo("late", Some("2026-08-01"), false),
            todo("today", Some("2026-08-04"), false),
            todo("later", Some("2026-08-09"), false),
        ];
        let d = build_digest(&todos, day("2026-08-04"));
        assert_eq!(d.overdue, vec!["late"]);
        assert_eq!(d.due_today, vec!["today"]);
    }

    #[test]
    fn ignores_done_deleted_and_undated() {
        let mut deleted = todo("deleted", Some("2026-08-01"), false);
        deleted.deleted_at = Some(chrono::Utc::now());
        let todos = vec![
            todo("finished", Some("2026-08-01"), true),
            deleted,
            todo("someday", None, false),
        ];
        let d = build_digest(&todos, day("2026-08-04"));
        assert!(d.is_empty(), "nothing actionable should produce no digest");
    }

    #[test]
    fn title_reads_naturally_for_each_combination() {
        let both = Digest {
            due_today: vec!["a".into(), "b".into()],
            overdue: vec!["c".into()],
        };
        assert_eq!(both.title(), "2 due today · 1 overdue");

        let only_overdue = Digest {
            due_today: vec![],
            overdue: vec!["c".into()],
        };
        assert_eq!(only_overdue.title(), "1 overdue");
    }

    #[test]
    fn body_leads_with_overdue_and_truncates() {
        let d = Digest {
            due_today: vec!["t1".into(), "t2".into(), "t3".into()],
            overdue: vec!["o1".into(), "o2".into()],
        };
        let body = d.body();
        assert!(body.starts_with("o1\no2"), "overdue must come first: {body}");
        assert!(body.contains("…and 1 more"));
    }

    fn buckets(list: &[&str]) -> Vec<String> {
        list.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn slack_buckets_are_independent() {
        let todos = vec![
            todo("t-today", Some("2026-08-04"), false),
            todo("t-tmrw", Some("2026-08-05"), false),
            todo("t-friday", Some("2026-08-07"), false),
        ];
        // Only "tomorrow" selected: neither today's task nor Friday's appears.
        let msg = build_slack_message(&todos, day("2026-08-04"), &buckets(&["tomorrow"])).unwrap();
        assert!(msg.contains("t-tmrw — due tomorrow"));
        assert!(!msg.contains("t-today"));
        assert!(!msg.contains("t-friday"));
        // "week" alone covers 2–7 days out, not today or tomorrow.
        let msg = build_slack_message(&todos, day("2026-08-04"), &buckets(&["week"])).unwrap();
        assert!(msg.contains("t-friday — due Fri Aug 7"));
        assert!(!msg.contains("t-today"));
        assert!(!msg.contains("t-tmrw"));
    }

    #[test]
    fn slack_all_buckets_cover_the_week_but_not_beyond() {
        let todos = vec![
            todo("today", Some("2026-08-04"), false),
            todo("tmrw", Some("2026-08-05"), false),
            todo("in-a-week", Some("2026-08-11"), false),
            todo("in-8-days", Some("2026-08-12"), false),
        ];
        let all = buckets(&["today", "tomorrow", "week"]);
        let msg = build_slack_message(&todos, day("2026-08-04"), &all).unwrap();
        assert!(msg.contains("today"));
        assert!(msg.contains("tmrw"));
        assert!(msg.contains("in-a-week"));
        assert!(!msg.contains("in-8-days"));
    }

    #[test]
    fn slack_always_includes_overdue_and_lists_it_first() {
        let todos = vec![
            todo("today", Some("2026-08-04"), false),
            todo("late", Some("2026-08-01"), false),
        ];
        let msg = build_slack_message(&todos, day("2026-08-04"), &buckets(&["today"])).unwrap();
        assert!(msg.starts_with("To Do: 1 overdue · 1 due soon"));
        let late_pos = msg.find("late — overdue since 2026-08-01").unwrap();
        let today_pos = msg.find("today — due today").unwrap();
        assert!(late_pos < today_pos, "overdue must come first: {msg}");
    }

    #[test]
    fn slack_empty_buckets_still_report_overdue() {
        let todos = vec![
            todo("late", Some("2026-08-01"), false),
            todo("today", Some("2026-08-04"), false),
        ];
        let msg = build_slack_message(&todos, day("2026-08-04"), &[]).unwrap();
        assert!(msg.contains("late"));
        assert!(!msg.contains("due today"));
    }

    #[test]
    fn slack_message_is_none_when_nothing_qualifies() {
        let mut archived = todo("archived", Some("2026-08-04"), false);
        archived.archived_at = Some(chrono::Utc::now());
        let todos = vec![
            todo("done", Some("2026-08-04"), true),
            todo("far-future", Some("2026-12-01"), false),
            todo("undated", None, false),
            archived,
        ];
        let all = buckets(&["today", "tomorrow", "week"]);
        assert_eq!(build_slack_message(&todos, day("2026-08-04"), &all), None);
    }

    #[test]
    fn fires_once_the_time_has_passed() {
        let now = Local.with_ymd_and_hms(2026, 8, 4, 8, 0, 0).unwrap();
        assert!(should_fire(now, 8, 0, None));
    }

    #[test]
    fn does_not_fire_before_the_time() {
        let now = Local.with_ymd_and_hms(2026, 8, 4, 7, 59, 0).unwrap();
        assert!(!should_fire(now, 8, 0, None));
    }

    #[test]
    fn does_not_fire_twice_in_one_day() {
        let now = Local.with_ymd_and_hms(2026, 8, 4, 12, 0, 0).unwrap();
        assert!(!should_fire(now, 8, 0, Some(day("2026-08-04"))));
    }

    #[test]
    fn fires_late_when_the_app_was_not_running_at_the_time() {
        // Opened the laptop at 3pm; the 8am digest should still arrive.
        let now = Local.with_ymd_and_hms(2026, 8, 4, 15, 0, 0).unwrap();
        assert!(should_fire(now, 8, 0, Some(day("2026-08-03"))));
    }
}
