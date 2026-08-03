// db.rs — everything that talks to Neon.
//
// The database is the shared source of truth between machines, but never the
// thing the UI reads from directly; `sync.rs` owns that relationship.
use crate::model::Todo;
use chrono::{DateTime, NaiveDate, Utc};
use sqlx::postgres::{PgPool, PgPoolOptions};
use sqlx::Row;
use std::time::Duration;

/// Tombstones stop propagating after this long and are cleaned up.
const TOMBSTONE_TTL_DAYS: i64 = 30;

const SCHEMA: &str = r#"
create table if not exists todos (
  id         text primary key,
  title      text not null,
  note       text not null default '',
  link       text,
  due        date,
  priority   text,
  done       boolean not null default false,
  tags       text[] not null default '{}',
  refs       text[] not null default '{}',
  created_at timestamptz not null default now(),
  updated_at timestamptz not null default now(),
  deleted_at timestamptz
);
create index if not exists todos_updated_at_idx on todos (updated_at);
"#;

/// Open a pool. Fails fast so a bad credential surfaces as an error the user
/// can see rather than a hang.
pub async fn connect(url: &str) -> Result<PgPool, String> {
    PgPoolOptions::new()
        .max_connections(2)
        .acquire_timeout(Duration::from_secs(15))
        .connect(url)
        .await
        .map_err(|e| e.to_string())
}

pub async fn ensure_schema(pool: &PgPool) -> Result<(), String> {
    sqlx::raw_sql(SCHEMA)
        .execute(pool)
        .await
        .map(|_| ())
        .map_err(|e| e.to_string())
}

fn row_to_todo(row: &sqlx::postgres::PgRow) -> Result<Todo, String> {
    let due: Option<NaiveDate> = row.try_get("due").map_err(|e| e.to_string())?;
    Ok(Todo {
        id: row.try_get("id").map_err(|e| e.to_string())?,
        title: row.try_get("title").map_err(|e| e.to_string())?,
        note: row.try_get("note").map_err(|e| e.to_string())?,
        link: row.try_get("link").map_err(|e| e.to_string())?,
        due: due.map(|d| d.format("%Y-%m-%d").to_string()),
        priority: row.try_get("priority").map_err(|e| e.to_string())?,
        done: row.try_get("done").map_err(|e| e.to_string())?,
        tags: row.try_get("tags").map_err(|e| e.to_string())?,
        refs: row.try_get("refs").map_err(|e| e.to_string())?,
        created_at: row.try_get("created_at").map_err(|e| e.to_string())?,
        updated_at: Some(row.try_get("updated_at").map_err(|e| e.to_string())?),
        deleted_at: row.try_get("deleted_at").map_err(|e| e.to_string())?,
        dirty: false,
    })
}

/// Fetch rows changed since `since` (all rows when `None`), plus the server's
/// current time to use as the next watermark.
///
/// Reading `now()` from the database rather than the machine is what keeps two
/// laptops with drifting clocks from losing each other's edits.
pub async fn pull(
    pool: &PgPool,
    since: Option<DateTime<Utc>>,
) -> Result<(Vec<Todo>, DateTime<Utc>), String> {
    let server_now: DateTime<Utc> = sqlx::query("select now() as now")
        .fetch_one(pool)
        .await
        .map_err(|e| e.to_string())?
        .try_get("now")
        .map_err(|e| e.to_string())?;

    let rows = match since {
        Some(ts) => sqlx::query("select * from todos where updated_at > $1")
            .bind(ts)
            .fetch_all(pool)
            .await,
        None => sqlx::query("select * from todos").fetch_all(pool).await,
    }
    .map_err(|e| e.to_string())?;

    let todos = rows.iter().map(row_to_todo).collect::<Result<Vec<_>, _>>()?;
    Ok((todos, server_now))
}

/// Write rows, stamping `updated_at` server-side.
///
/// Last push wins: callers pull and merge first, so anything reaching here is
/// the value this machine intends to be current.
pub async fn push(pool: &PgPool, todos: &[Todo]) -> Result<(), String> {
    if todos.is_empty() {
        return Ok(());
    }
    let mut tx = pool.begin().await.map_err(|e| e.to_string())?;

    for t in todos {
        let due: Option<NaiveDate> = match &t.due {
            Some(s) if !s.is_empty() => NaiveDate::parse_from_str(s, "%Y-%m-%d").ok(),
            _ => None,
        };
        sqlx::query(
            r#"
            insert into todos
              (id, title, note, link, due, priority, done, tags, refs, created_at, updated_at, deleted_at)
            values ($1,$2,$3,$4,$5,$6,$7,$8,$9,$10, now(), $11)
            on conflict (id) do update set
              title      = excluded.title,
              note       = excluded.note,
              link       = excluded.link,
              due        = excluded.due,
              priority   = excluded.priority,
              done       = excluded.done,
              tags       = excluded.tags,
              refs       = excluded.refs,
              created_at = excluded.created_at,
              deleted_at = excluded.deleted_at,
              updated_at = now()
            "#,
        )
        .bind(&t.id)
        .bind(&t.title)
        .bind(&t.note)
        .bind(&t.link)
        .bind(due)
        .bind(&t.priority)
        .bind(t.done)
        .bind(&t.tags)
        .bind(&t.refs)
        .bind(t.created_at)
        .bind(t.deleted_at)
        .execute(&mut *tx)
        .await
        .map_err(|e| e.to_string())?;
    }

    tx.commit().await.map_err(|e| e.to_string())
}

/// Drop tombstones old enough that every machine has certainly seen them.
pub async fn purge_tombstones(pool: &PgPool) -> Result<u64, String> {
    let cutoff = Utc::now() - chrono::Duration::days(TOMBSTONE_TTL_DAYS);
    sqlx::query("delete from todos where deleted_at is not null and deleted_at < $1")
        .bind(cutoff)
        .execute(pool)
        .await
        .map(|r| r.rows_affected())
        .map_err(|e| e.to_string())
}

/* ---------- integration tests ---------- */

// These hit a real database. Set TODO_TEST_DATABASE_URL to run them:
//   cargo test -- --ignored
// They operate only on rows whose id starts with `zz-test-`, so a live list is
// never touched.
#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::Todo;

    const TEST_PREFIX: &str = "zz-test-";

    fn test_url() -> Option<String> {
        std::env::var("TODO_TEST_DATABASE_URL").ok().filter(|s| !s.is_empty())
    }

    async fn cleanup(pool: &PgPool) {
        let _ = sqlx::query("delete from todos where id like $1")
            .bind(format!("{TEST_PREFIX}%"))
            .execute(pool)
            .await;
    }

    fn sample(id: &str) -> Todo {
        Todo {
            id: format!("{TEST_PREFIX}{id}"),
            title: "round-trip me".into(),
            note: "note with 'quotes' and — punctuation".into(),
            link: Some("https://example.com/x?a=1&b=2".into()),
            due: Some("2026-06-12".into()),
            priority: Some("high".into()),
            done: true,
            tags: vec!["ai".into(), "multi word".into()],
            refs: vec!["zz-test-other".into()],
            created_at: DateTime::from_timestamp(1_700_000_000, 0).unwrap(),
            updated_at: None,
            deleted_at: None,
            dirty: true,
        }
    }

    #[tokio::test]
    #[ignore]
    async fn every_field_survives_a_push_and_pull() {
        let Some(url) = test_url() else { return };
        let pool = connect(&url).await.expect("connect");
        ensure_schema(&pool).await.expect("schema");
        cleanup(&pool).await;

        let original = sample("roundtrip");
        push(&pool, std::slice::from_ref(&original)).await.expect("push");

        let (rows, _now) = pull(&pool, None).await.expect("pull");
        let got = rows
            .iter()
            .find(|t| t.id == original.id)
            .expect("pushed row must come back");

        assert_eq!(got.title, original.title);
        assert_eq!(got.note, original.note);
        assert_eq!(got.link, original.link);
        assert_eq!(got.due, original.due, "date must survive as YYYY-MM-DD");
        assert_eq!(got.priority, original.priority);
        assert_eq!(got.done, original.done);
        assert_eq!(got.tags, original.tags, "text[] must round-trip, spaces included");
        assert_eq!(got.refs, original.refs);
        assert_eq!(got.created_at, original.created_at);
        assert!(got.updated_at.is_some(), "server must stamp updated_at");
        assert!(!got.dirty, "rows from the database are never dirty");

        cleanup(&pool).await;
    }

    #[tokio::test]
    #[ignore]
    async fn the_watermark_filters_out_rows_already_seen() {
        let Some(url) = test_url() else { return };
        let pool = connect(&url).await.expect("connect");
        ensure_schema(&pool).await.expect("schema");
        cleanup(&pool).await;

        push(&pool, &[sample("first")]).await.expect("push first");
        let (_all, watermark) = pull(&pool, None).await.expect("pull all");

        // Nothing new since the watermark.
        let (empty, _) = pull(&pool, Some(watermark)).await.expect("pull since");
        assert!(
            !empty.iter().any(|t| t.id.starts_with(TEST_PREFIX)),
            "a row already seen must not come back"
        );

        // A later write does show up.
        push(&pool, &[sample("second")]).await.expect("push second");
        let (fresh, _) = pull(&pool, Some(watermark)).await.expect("pull since");
        assert!(
            fresh.iter().any(|t| t.id == format!("{TEST_PREFIX}second")),
            "a row written after the watermark must be returned"
        );

        cleanup(&pool).await;
    }

    #[tokio::test]
    #[ignore]
    async fn a_tombstone_propagates_as_a_deleted_row() {
        let Some(url) = test_url() else { return };
        let pool = connect(&url).await.expect("connect");
        ensure_schema(&pool).await.expect("schema");
        cleanup(&pool).await;

        let mut t = sample("tomb");
        push(&pool, std::slice::from_ref(&t)).await.expect("push live");

        t.deleted_at = Some(Utc::now());
        push(&pool, std::slice::from_ref(&t)).await.expect("push tombstone");

        let (rows, _) = pull(&pool, None).await.expect("pull");
        let got = rows.iter().find(|r| r.id == t.id).expect("tombstone must be pulled");
        assert!(got.is_deleted(), "delete must travel as a tombstone, not a missing row");

        cleanup(&pool).await;
    }

    #[tokio::test]
    #[ignore]
    async fn a_null_heavy_row_round_trips() {
        let Some(url) = test_url() else { return };
        let pool = connect(&url).await.expect("connect");
        ensure_schema(&pool).await.expect("schema");
        cleanup(&pool).await;

        let mut bare = sample("bare");
        bare.link = None;
        bare.due = None;
        bare.priority = None;
        bare.tags = vec![];
        bare.refs = vec![];
        bare.note = String::new();
        push(&pool, std::slice::from_ref(&bare)).await.expect("push");

        let (rows, _) = pull(&pool, None).await.expect("pull");
        let got = rows.iter().find(|r| r.id == bare.id).expect("row");
        assert_eq!(got.link, None);
        assert_eq!(got.due, None);
        assert_eq!(got.priority, None);
        assert!(got.tags.is_empty());
        assert!(got.refs.is_empty());
        assert_eq!(got.note, "");

        cleanup(&pool).await;
    }
}
