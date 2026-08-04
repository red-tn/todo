// model.rs — the Todo record, shared by the local cache, the database, and the frontend.
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

/// A single todo.
///
/// Serialized in camelCase so the JSON matches what `store.js` has always
/// written; the pre-sync `todos.json` files load unchanged because every field
/// added for sync has a serde default.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Todo {
    pub id: String,
    pub title: String,
    #[serde(default)]
    pub note: String,
    /// Due date as "YYYY-MM-DD", matching the frontend's `<input type="date">`.
    #[serde(default)]
    pub due: Option<String>,
    /// "low" | "med" | "high" | None.
    #[serde(default)]
    pub priority: Option<String>,
    #[serde(default)]
    pub done: bool,
    #[serde(default)]
    pub tags: Vec<String>,
    #[serde(default = "Utc::now")]
    pub created_at: DateTime<Utc>,

    // ---- recurrence ----
    /// "daily" | "weekly" | "monthly" | "yearly" | None for a one-off.
    #[serde(default)]
    pub recurrence: Option<String>,
    /// Combines with `recurrence` to give "every 2 weeks". Always at least 1.
    #[serde(default = "one")]
    pub recurrence_interval: i32,

    // ---- sync metadata ----
    /// Last modification time. Set locally on edit, then replaced by the
    /// server's value once the row has been pushed.
    #[serde(default)]
    pub updated_at: Option<DateTime<Utc>>,
    /// Soft delete. `Some` means this is a tombstone and the frontend never
    /// sees it.
    #[serde(default)]
    pub deleted_at: Option<DateTime<Utc>>,
    /// Set once a completed task has aged out of the working list. Archived
    /// tasks still sync and can be restored; they are simply not shown.
    #[serde(default)]
    pub archived_at: Option<DateTime<Utc>>,
    /// Local-only: this row has unpushed changes. Never sent to the database.
    #[serde(default, skip_serializing_if = "is_false")]
    pub dirty: bool,
}

fn is_false(b: &bool) -> bool {
    !*b
}

/// Default interval for caches written before recurrence existed.
fn one() -> i32 {
    1
}

impl Todo {
    /// Effective modification time, falling back to creation time for rows
    /// written before sync existed.
    pub fn stamp(&self) -> DateTime<Utc> {
        self.updated_at.unwrap_or(self.created_at)
    }

    pub fn is_deleted(&self) -> bool {
        self.deleted_at.is_some()
    }

    pub fn is_archived(&self) -> bool {
        self.archived_at.is_some()
    }

    /// Shown in the working list: not deleted, not archived.
    pub fn is_live(&self) -> bool {
        !self.is_deleted() && !self.is_archived()
    }
}
