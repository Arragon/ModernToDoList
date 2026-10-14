//! M9: Smart Views domain logic (RD-M9-014~021).
//!
//! Pure, deterministic evaluation of the built-in smart views over an
//! in-memory task projection ([`ViewTask`]). All date arithmetic uses
//! `chrono` with an INJECTED "now" (`NaiveDate`) so tests can pin exact
//! boundaries (midnight, today vs tomorrow, week rollover).
//!
//! Views:
//! - Today:        unfinished AND (due_date == today OR start_date <= today)
//! - Upcoming:     unfinished with due date, grouped Today / Tomorrow / This Week / Later
//! - Overdue:      due_date < today AND status not in {Completed, Cancelled}
//! - Unscheduled:  no start_date AND no due_date (unfinished)
//! - Completed:    status Completed, sorted by completed_date descending
//! - Flagged:      priority >= High OR custom flag set
//! - ByParticipant / ByTag: deterministic grouping views
//!
//! This module never touches SQLite or XML — the IPC layer (commands/views.rs)
//! feeds it rows from the disposable index.

use chrono::{Datelike, Duration, NaiveDate};
use serde::{Deserialize, Serialize};

use super::task::Task;

/// OLE Automation date epoch: 1899-12-30 (day 0). TDL stores dates as
/// floating-point days since this epoch.
const OLE_EPOCH: NaiveDate = match NaiveDate::from_ymd_opt(1899, 12, 30) {
    Some(d) => d,
    None => panic!("valid epoch"),
};

/// Convert a TDL OLE Automation date (days since 1899-12-30) to a calendar date.
///
/// Returns `None` for values out of the representable range.
pub fn ole_to_naive_date(ole: f64) -> Option<NaiveDate> {
    if !ole.is_finite() {
        return None;
    }
    let days = ole.floor() as i64;
    OLE_EPOCH.checked_add_signed(Duration::days(days))
}

/// Convert a calendar date to a TDL OLE Automation date (midnight).
pub fn naive_date_to_ole(date: NaiveDate) -> f64 {
    (date - OLE_EPOCH).num_days() as f64
}

/// Parse an index TEXT date cell. The indexer stores OLE floats as text,
/// but ISO strings ("2024-01-15") are also accepted for robustness.
pub fn parse_index_date(raw: Option<&str>) -> Option<NaiveDate> {
    let raw = raw?.trim();
    if raw.is_empty() {
        return None;
    }
    if let Ok(ole) = raw.parse::<f64>() {
        return ole_to_naive_date(ole);
    }
    NaiveDate::parse_from_str(raw, "%Y-%m-%d").ok()
}

/// Priority threshold for the Flagged view: 7 = High on TDL's 0-10 scale
/// (0=None, 1-3=Low, 4-6=Medium, 7-9=High, 10=Very High).
pub const FLAGGED_PRIORITY_THRESHOLD: u8 = 7;

/// Group key for tasks without participants (ByParticipant view).
pub const UNASSIGNED_GROUP: &str = "(unassigned)";
/// Group key for tasks without tags (ByTag view).
pub const UNTAGGED_GROUP: &str = "(untagged)";

/// The set of smart view kinds.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SmartViewKind {
    Today,
    Upcoming,
    Overdue,
    Unscheduled,
    Completed,
    Flagged,
    ByParticipant,
    ByTag,
}

impl SmartViewKind {
    /// Parse from the IPC string form.
    pub fn parse(s: &str) -> Option<Self> {
        Some(match s {
            "today" => Self::Today,
            "upcoming" => Self::Upcoming,
            "overdue" => Self::Overdue,
            "unscheduled" => Self::Unscheduled,
            "completed" => Self::Completed,
            "flagged" => Self::Flagged,
            "by_participant" => Self::ByParticipant,
            "by_tag" => Self::ByTag,
            _ => return None,
        })
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Today => "today",
            Self::Upcoming => "upcoming",
            Self::Overdue => "overdue",
            Self::Unscheduled => "unscheduled",
            Self::Completed => "completed",
            Self::Flagged => "flagged",
            Self::ByParticipant => "by_participant",
            Self::ByTag => "by_tag",
        }
    }
}

/// A task projection for view evaluation.
///
/// Defined locally so smart views do not depend on other milestones'
/// domain types (M6 participant structs etc.). The IPC layer builds these
/// from `task_index` rows (+ tags/participants tables).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ViewTask {
    /// "document_id:task_id"
    pub task_key: String,
    pub document_id: String,
    pub task_id: String,
    pub title: String,
    /// Raw status string as stored in the index (e.g. "NotStarted").
    pub status: String,
    pub percent_done: u8,
    /// 0-10 TDL scale.
    pub priority: u8,
    pub start_date: Option<NaiveDate>,
    pub due_date: Option<NaiveDate>,
    pub completed_date: Option<NaiveDate>,
    pub tags: Vec<String>,
    pub participants: Vec<String>,
    /// Custom user flag (separate from priority-based flagging).
    pub flagged: bool,
}

impl ViewTask {
    /// Build from a full domain task (used when a document is in memory).
    pub fn from_task(document_id: &str, task: &Task) -> Self {
        Self {
            task_key: format!("{}:{}", document_id, task.id),
            document_id: document_id.to_string(),
            task_id: task.id.to_string(),
            title: task.title.clone(),
            status: format!("{:?}", task.status()),
            percent_done: task.percent_done,
            priority: task.priority.value(),
            start_date: task.start_date.and_then(ole_to_naive_date),
            due_date: task.due_date.and_then(ole_to_naive_date),
            completed_date: task.completion_date.and_then(ole_to_naive_date),
            tags: task.categories.iter().map(|c| c.name.clone()).collect(),
            participants: task.allocated_to.clone(),
            flagged: false,
        }
    }

    /// Completed per TDL semantics (100% done or explicit Done status).
    pub fn is_completed(&self) -> bool {
        self.percent_done >= 100 || self.status == "Done" || self.status == "Completed"
    }

    /// Cancelled tasks (legacy status string; TDL 2.0 has no native cancel).
    pub fn is_cancelled(&self) -> bool {
        self.status == "Cancelled"
    }

    /// Unfinished = neither completed nor cancelled.
    pub fn is_active(&self) -> bool {
        !self.is_completed() && !self.is_cancelled()
    }
}

/// Date buckets for the Upcoming view.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum UpcomingBucket {
    Today,
    Tomorrow,
    ThisWeek,
    Later,
}

/// Grouped result of the Upcoming view.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct UpcomingGroups {
    pub today: Vec<ViewTask>,
    pub tomorrow: Vec<ViewTask>,
    pub this_week: Vec<ViewTask>,
    pub later: Vec<ViewTask>,
}

impl UpcomingGroups {
    pub fn is_empty(&self) -> bool {
        self.today.is_empty()
            && self.tomorrow.is_empty()
            && self.this_week.is_empty()
            && self.later.is_empty()
    }

    pub fn total(&self) -> usize {
        self.today.len() + self.tomorrow.len() + self.this_week.len() + self.later.len()
    }
}

/// A named group of tasks (ByParticipant / ByTag views).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TaskGroup {
    pub name: String,
    pub tasks: Vec<ViewTask>,
}

/// Last day of the current week, given Monday as the week start.
/// On a Sunday (week end) this equals `today`.
pub fn end_of_week(today: NaiveDate) -> NaiveDate {
    // num_days_from_monday(): Mon=0 .. Sun=6
    let days_to_sunday = 6 - today.weekday().num_days_from_monday();
    today + Duration::days(i64::from(days_to_sunday))
}

/// Classify a due date into an Upcoming bucket relative to `today`.
/// Returns `None` for due dates in the past (they belong to Overdue).
pub fn upcoming_bucket(due: NaiveDate, today: NaiveDate) -> Option<UpcomingBucket> {
    if due < today {
        return None;
    }
    if due == today {
        return Some(UpcomingBucket::Today);
    }
    if due == today + Duration::days(1) {
        return Some(UpcomingBucket::Tomorrow);
    }
    if due <= end_of_week(today) {
        return Some(UpcomingBucket::ThisWeek);
    }
    Some(UpcomingBucket::Later)
}

/// Today view: unfinished tasks with due_date == today OR start_date <= today.
///
/// Sorted by due date (None last), then task_key for determinism.
pub fn today_view<'a>(tasks: &'a [ViewTask], today: NaiveDate) -> Vec<&'a ViewTask> {
    let mut hits: Vec<&ViewTask> = tasks
        .iter()
        .filter(|t| t.is_active())
        .filter(|t| t.due_date == Some(today) || t.start_date.map_or(false, |s| s <= today))
        .collect();
    sort_by_due_then_key(&mut hits);
    hits
}

/// Upcoming view: unfinished tasks with a due date, grouped into
/// Today / Tomorrow / This Week / Later. Past-due tasks are excluded
/// (they are reported by the Overdue view).
pub fn upcoming_view(tasks: &[ViewTask], today: NaiveDate) -> UpcomingGroups {
    let mut groups = UpcomingGroups {
        today: Vec::new(),
        tomorrow: Vec::new(),
        this_week: Vec::new(),
        later: Vec::new(),
    };
    for t in tasks {
        if !t.is_active() {
            continue;
        }
        let Some(due) = t.due_date else { continue };
        match upcoming_bucket(due, today) {
            Some(UpcomingBucket::Today) => groups.today.push(t.clone()),
            Some(UpcomingBucket::Tomorrow) => groups.tomorrow.push(t.clone()),
            Some(UpcomingBucket::ThisWeek) => groups.this_week.push(t.clone()),
            Some(UpcomingBucket::Later) => groups.later.push(t.clone()),
            None => {}
        }
    }
    for g in [&mut groups.today, &mut groups.tomorrow, &mut groups.this_week, &mut groups.later] {
        g.sort_by(|a, b| a.task_key.cmp(&b.task_key));
    }
    groups
}

/// Overdue view: due_date < today AND status not in {Completed, Cancelled}.
/// Sorted by due date ascending (most overdue first), then task_key.
pub fn overdue_view<'a>(tasks: &'a [ViewTask], today: NaiveDate) -> Vec<&'a ViewTask> {
    let mut hits: Vec<&ViewTask> = tasks
        .iter()
        .filter(|t| t.is_active())
        .filter(|t| t.due_date.map_or(false, |d| d < today))
        .collect();
    sort_by_due_then_key(&mut hits);
    hits
}

/// Unscheduled view: no start_date AND no due_date (unfinished only).
/// Preserves document order (stable filter), which is deterministic.
pub fn unscheduled_view<'a>(tasks: &'a [ViewTask]) -> Vec<&'a ViewTask> {
    tasks
        .iter()
        .filter(|t| t.is_active() && t.start_date.is_none() && t.due_date.is_none())
        .collect()
}

/// Completed view: status Completed, sorted by completed_date descending
/// (tasks without a completion date sort last), then task_key ascending.
pub fn completed_view(tasks: &[ViewTask]) -> Vec<ViewTask> {
    let mut hits: Vec<ViewTask> = tasks.iter().filter(|t| t.is_completed()).cloned().collect();
    hits.sort_by(|a, b| {
        b.completed_date
            .cmp(&a.completed_date)
            .then_with(|| a.task_key.cmp(&b.task_key))
    });
    hits
}

/// Flagged view: priority >= High (7) OR the custom flag is set.
/// Sorted by priority descending, then task_key.
pub fn flagged_view(tasks: &[ViewTask]) -> Vec<ViewTask> {
    let mut hits: Vec<ViewTask> = tasks
        .iter()
        .filter(|t| t.flagged || t.priority >= FLAGGED_PRIORITY_THRESHOLD)
        .cloned()
        .collect();
    hits.sort_by(|a, b| b.priority.cmp(&a.priority).then_with(|| a.task_key.cmp(&b.task_key)));
    hits
}

/// ByParticipant grouping: every participant name gets a group; tasks with
/// several participants appear in each of their groups. Tasks without any
/// participant land in [`UNASSIGNED_GROUP`]. Groups sorted by name; tasks
/// inside a group sorted by task_key.
pub fn by_participant_view(tasks: &[ViewTask]) -> Vec<TaskGroup> {
    group_by(tasks, |t| {
        if t.participants.is_empty() {
            vec![UNASSIGNED_GROUP.to_string()]
        } else {
            t.participants.clone()
        }
    })
}

/// ByTag grouping: same semantics as [`by_participant_view`] over tags,
/// with [`UNTAGGED_GROUP`] as the catch-all.
pub fn by_tag_view(tasks: &[ViewTask]) -> Vec<TaskGroup> {
    group_by(tasks, |t| {
        if t.tags.is_empty() {
            vec![UNTAGGED_GROUP.to_string()]
        } else {
            t.tags.clone()
        }
    })
}

fn group_by<F>(tasks: &[ViewTask], keys: F) -> Vec<TaskGroup>
where
    F: Fn(&ViewTask) -> Vec<String>,
{
    use std::collections::BTreeMap;
    let mut map: BTreeMap<String, Vec<ViewTask>> = BTreeMap::new();
    for t in tasks {
        for key in keys(t) {
            map.entry(key).or_default().push(t.clone());
        }
    }
    map.into_iter()
        .map(|(name, mut group)| {
            group.sort_by(|a, b| a.task_key.cmp(&b.task_key));
            TaskGroup { name, tasks: group }
        })
        .collect()
}

fn sort_by_due_then_key(hits: &mut [&ViewTask]) {
    hits.sort_by(|a, b| {
        due_order(a.due_date, b.due_date).then_with(|| a.task_key.cmp(&b.task_key))
    });
}

/// Due-date ordering with `None` sorting LAST (Option's default puts None first).
fn due_order(a: Option<NaiveDate>, b: Option<NaiveDate>) -> std::cmp::Ordering {
    match (a, b) {
        (None, None) => std::cmp::Ordering::Equal,
        (None, Some(_)) => std::cmp::Ordering::Greater,
        (Some(_), None) => std::cmp::Ordering::Less,
        (Some(x), Some(y)) => x.cmp(&y),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Weekday;

    fn d(y: i32, m: u32, day: u32) -> NaiveDate {
        NaiveDate::from_ymd_opt(y, m, day).unwrap()
    }

    fn task(key: &str) -> ViewTask {
        ViewTask {
            task_key: key.to_string(),
            document_id: "doc".into(),
            task_id: key.into(),
            title: format!("Task {key}"),
            status: "NotStarted".into(),
            percent_done: 0,
            priority: 5,
            start_date: None,
            due_date: None,
            completed_date: None,
            tags: vec![],
            participants: vec![],
            flagged: false,
        }
    }

    #[test]
    fn ole_date_round_trip() {
        // 2024-01-15 is OLE 45306 (days since 1899-12-30).
        let date = d(2024, 1, 15);
        let ole = naive_date_to_ole(date);
        assert_eq!(ole, 45306.0);
        assert_eq!(ole_to_naive_date(ole), Some(date));
        assert_eq!(ole_to_naive_date(45306.75), Some(date)); // intraday time
        assert_eq!(ole_to_naive_date(f64::NAN), None);
    }

    #[test]
    fn parse_index_date_accepts_ole_and_iso() {
        assert_eq!(parse_index_date(Some("45306")), Some(d(2024, 1, 15)));
        assert_eq!(parse_index_date(Some("2024-01-15")), Some(d(2024, 1, 15)));
        assert_eq!(parse_index_date(Some("")), None);
        assert_eq!(parse_index_date(None), None);
    }

    #[test]
    fn today_view_boundary_midnight() {
        // "today" is a NaiveDate — midnight belongs to the same day on both sides.
        let today = d(2024, 3, 10);
        let due_today = { let mut t = task("a"); t.due_date = Some(today); t };
        let due_tomorrow = { let mut t = task("b"); t.due_date = Some(today + Duration::days(1)); t };
        let started_yesterday = { let mut t = task("c"); t.start_date = Some(today - Duration::days(1)); t };
        let starts_tomorrow = { let mut t = task("d"); t.start_date = Some(today + Duration::days(1)); t };
        let done_today = { let mut t = task("e"); t.due_date = Some(today); t.percent_done = 100; t };

        let tasks = vec![due_today, due_tomorrow, started_yesterday, starts_tomorrow, done_today];
        let view = today_view(&tasks, today);
        let keys: Vec<&str> = view.iter().map(|t| t.task_key.as_str()).collect();
        assert_eq!(keys, vec!["a", "c"], "due-today + already-started, not finished/future");
    }

    #[test]
    fn overdue_excludes_completed_and_cancelled() {
        let today = d(2024, 3, 10);
        let overdue_open = { let mut t = task("a"); t.due_date = Some(today - Duration::days(2)); t };
        let overdue_done = {
            let mut t = task("b");
            t.due_date = Some(today - Duration::days(1));
            t.percent_done = 100;
            t
        };
        let overdue_cancelled = {
            let mut t = task("c");
            t.due_date = Some(today - Duration::days(1));
            t.status = "Cancelled".into();
            t
        };
        let due_today = { let mut t = task("d"); t.due_date = Some(today); t };
        let tasks = vec![overdue_open, overdue_done, overdue_cancelled, due_today];
        let view = overdue_view(&tasks, today);
        let keys: Vec<&str> = view.iter().map(|t| t.task_key.as_str()).collect();
        assert_eq!(keys, vec!["a"]);
    }

    #[test]
    fn upcoming_buckets_and_week_rollover() {
        // 2024-03-13 is a Wednesday; week ends Sunday 2024-03-17.
        let today = d(2024, 3, 13);
        assert_eq!(today.weekday(), Weekday::Wed);
        assert_eq!(end_of_week(today), d(2024, 3, 17));

        let mk = |key: &str, due: NaiveDate| { let mut t = task(key); t.due_date = Some(due); t };
        let tasks = vec![
            mk("t0", today),
            mk("t1", today + Duration::days(1)),
            mk("t4", d(2024, 3, 17)), // Sunday -> This Week
            mk("n1", d(2024, 3, 18)), // Monday -> Later (week rollover)
            mk("past", today - Duration::days(1)), // excluded
        ];
        let groups = upcoming_view(&tasks, today);
        assert_eq!(groups.today.len(), 1);
        assert_eq!(groups.tomorrow.len(), 1);
        assert_eq!(groups.this_week.len(), 1);
        assert_eq!(groups.this_week[0].task_key, "t4");
        assert_eq!(groups.later.len(), 1);
        assert_eq!(groups.later[0].task_key, "n1");
        assert_eq!(groups.total(), 4);
    }

    #[test]
    fn week_rollover_on_sunday() {
        // On Sunday, end_of_week == today: nothing beyond tomorrow can be
        // "This Week" — Monday rolls into the next week (Tomorrow bucket,
        // since from Sunday's perspective Monday is tomorrow).
        let sunday = d(2024, 3, 17);
        assert_eq!(end_of_week(sunday), sunday);
        let t_mon = { let mut t = task("m"); t.due_date = Some(d(2024, 3, 18)); t };
        let t_wed = { let mut t = task("w"); t.due_date = Some(d(2024, 3, 20)); t };
        let groups = upcoming_view(&[t_mon, t_wed], sunday);
        assert_eq!(groups.tomorrow.len(), 1, "Monday is Tomorrow on Sunday");
        assert!(groups.this_week.is_empty(), "week ended: nothing is This Week");
        assert_eq!(groups.later.len(), 1, "Wednesday rolled into next week");
    }

    #[test]
    fn tomorrow_boundary_is_exact() {
        let today = d(2024, 3, 13);
        assert_eq!(upcoming_bucket(today, today), Some(UpcomingBucket::Today));
        assert_eq!(upcoming_bucket(today + Duration::days(1), today), Some(UpcomingBucket::Tomorrow));
        assert_eq!(upcoming_bucket(today - Duration::days(1), today), None);
    }

    #[test]
    fn unscheduled_requires_both_dates_missing() {
        let open = task("a");
        let with_start = { let mut t = task("b"); t.start_date = Some(d(2024, 1, 1)); t };
        let with_due = { let mut t = task("c"); t.due_date = Some(d(2024, 1, 1)); t };
        let done = { let mut t = task("d"); t.percent_done = 100; t };
        let tasks = vec![open, with_start, with_due, done];
        let view = unscheduled_view(&tasks);
        assert_eq!(view.len(), 1);
        assert_eq!(view[0].task_key, "a");
    }

    #[test]
    fn completed_sorted_by_completed_date_desc() {
        let old = { let mut t = task("a"); t.percent_done = 100; t.completed_date = Some(d(2024, 1, 1)); t };
        let new = { let mut t = task("b"); t.percent_done = 100; t.completed_date = Some(d(2024, 6, 1)); t };
        let no_date = { let mut t = task("c"); t.percent_done = 100; t };
        let open = task("d");
        let view = completed_view(&[old, new, no_date, open]);
        let keys: Vec<&str> = view.iter().map(|t| t.task_key.as_str()).collect();
        assert_eq!(keys, vec!["b", "a", "c"]);
    }

    #[test]
    fn flagged_by_priority_or_custom_flag() {
        let high = { let mut t = task("a"); t.priority = 7; t };
        let vhigh = { let mut t = task("b"); t.priority = 10; t };
        let medium_flagged = { let mut t = task("c"); t.priority = 5; t.flagged = true; t };
        let medium = { let mut t = task("d"); t.priority = 5; t };
        let view = flagged_view(&[high, vhigh, medium_flagged, medium]);
        let keys: Vec<&str> = view.iter().map(|t| t.task_key.as_str()).collect();
        assert_eq!(keys, vec!["b", "a", "c"]);
    }

    #[test]
    fn by_participant_groups_and_unassigned() {
        let t1 = { let mut t = task("a"); t.participants = vec!["Alice".into(), "Bob".into()]; t };
        let t2 = { let mut t = task("b"); t.participants = vec!["Bob".into()]; t };
        let t3 = task("c");
        let groups = by_participant_view(&[t1, t2, t3]);
        let names: Vec<&str> = groups.iter().map(|g| g.name.as_str()).collect();
        assert_eq!(names, vec!["(unassigned)", "Alice", "Bob"]);
        let bob = groups.iter().find(|g| g.name == "Bob").unwrap();
        assert_eq!(bob.tasks.len(), 2);
    }

    #[test]
    fn by_tag_groups() {
        let t1 = { let mut t = task("a"); t.tags = vec!["work".into()]; t };
        let t2 = { let mut t = task("b"); t.tags = vec!["home".into(), "work".into()]; t };
        let t3 = task("c");
        let groups = by_tag_view(&[t1, t2, t3]);
        let names: Vec<&str> = groups.iter().map(|g| g.name.as_str()).collect();
        assert_eq!(names, vec!["(untagged)", "home", "work"]);
    }

    #[test]
    fn smart_view_kind_parse_roundtrip() {
        for k in [
            SmartViewKind::Today, SmartViewKind::Upcoming, SmartViewKind::Overdue,
            SmartViewKind::Unscheduled, SmartViewKind::Completed, SmartViewKind::Flagged,
            SmartViewKind::ByParticipant, SmartViewKind::ByTag,
        ] {
            assert_eq!(SmartViewKind::parse(k.as_str()), Some(k));
        }
        assert_eq!(SmartViewKind::parse("bogus"), None);
    }

    #[test]
    fn view_task_from_domain_task() {
        let mut t = Task::new(super::super::types::TaskId::new("42"));
        t.title = "中文任务".into();
        t.due_date = Some(45306.0); // 2024-01-15
        t.percent_done = 100;
        let vt = ViewTask::from_task("doc-1", &t);
        assert_eq!(vt.due_date, Some(d(2024, 1, 15)));
        assert!(vt.is_completed());
        assert_eq!(vt.task_key, "doc-1:42");
    }
}
