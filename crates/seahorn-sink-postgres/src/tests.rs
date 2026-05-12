//! Integration tests for PostgresSink.
//!
//! Requires Docker — each test spins up a throwaway Postgres container via testcontainers.
//! Run with: cargo test -p seahorn-sink-postgres -- --test-threads=1

use seahorn_core::{ChangeSet, Cursor, EntityChange, Sink, Step, Value};
use sqlx::{PgPool, Row};
use testcontainers::runners::AsyncRunner;
use testcontainers_modules::postgres::Postgres;

use crate::PostgresSink;

// ── helpers ─────────────────────────────────────────────────────────────────

/// Spin up a fresh Postgres container and return a connected sink.
/// Returns `None` if Docker is not available (CI without Docker, local dev without daemon).
async fn try_make_sink(cursor_name: &str) -> Option<(PostgresSink, testcontainers::ContainerAsync<Postgres>)> {
    let container = match Postgres::default().start().await {
        Ok(c) => c,
        Err(e) => {
            eprintln!("skipping integration test — Docker unavailable: {e}");
            return None;
        }
    };
    let port = container.get_host_port_ipv4(5432).await.expect("get port");
    let url = format!("postgres://postgres:postgres@127.0.0.1:{port}/postgres");
    let sink = PostgresSink::connect(&url, cursor_name).await.expect("connect sink");
    Some((sink, container))
}

macro_rules! require_docker {
    ($name:ident) => {
        let ($name, _container) = match try_make_sink(stringify!($name)).await {
            Some(pair) => pair,
            None => return,
        };
    };
}

/// Build a minimal changeset for testing.
fn changeset(slot: u64, cursor_bytes: &[u8], step: Step, changes: Vec<EntityChange>) -> ChangeSet {
    ChangeSet { slot, cursor: Cursor(cursor_bytes.to_vec()), step, changes }
}

fn upsert(entity_type: &'static str, id: &str, fields: Vec<(&'static str, Value)>) -> EntityChange {
    EntityChange::Upsert { entity_type, id: id.to_string(), fields }
}

fn delete(entity_type: &'static str, id: &str) -> EntityChange {
    EntityChange::Delete { entity_type, id: id.to_string() }
}

async fn row_count(pool: &PgPool) -> i64 {
    sqlx::query("SELECT COUNT(*) FROM entity_changes")
        .fetch_one(pool)
        .await
        .unwrap()
        .get(0)
}

async fn status_at_slot(pool: &PgPool, slot: i64) -> Vec<String> {
    sqlx::query("SELECT commitment_status FROM entity_changes WHERE slot = $1 ORDER BY id")
        .bind(slot)
        .fetch_all(pool)
        .await
        .unwrap()
        .into_iter()
        .map(|r| r.get::<String, _>("commitment_status"))
        .collect()
}

// ── tests ────────────────────────────────────────────────────────────────────

/// Writing a NEW changeset creates rows with status='NEW'.
#[tokio::test]
async fn test_apply_new() {
    require_docker!(sink);

    let cs = changeset(
        100,
        b"cursor1",
        Step::New,
        vec![
            upsert("Buy", "buy-1", vec![("mint", Value::String("PUMP".into())), ("sol_cost", Value::U64(1_000))]),
            upsert("Buy", "buy-2", vec![("mint", Value::String("PUMP".into())), ("sol_cost", Value::U64(2_000))]),
        ],
    );
    sink.apply(&cs).await.expect("apply");

    let count = row_count(&sink.pool).await;
    assert_eq!(count, 2, "expected 2 rows after NEW changeset");

    let statuses = status_at_slot(&sink.pool, 100).await;
    assert!(statuses.iter().all(|s| s == "NEW"));
}

/// A Delete entity change inserts a tombstone row with NULL fields.
#[tokio::test]
async fn test_apply_delete_tombstone() {
    require_docker!(sink);

    let cs = changeset(101, b"cursor-del", Step::New, vec![delete("Buy", "buy-1")]);
    sink.apply(&cs).await.expect("apply");

    let row = sqlx::query("SELECT fields FROM entity_changes WHERE entity_id = 'buy-1'")
        .fetch_one(&sink.pool)
        .await
        .unwrap();
    let fields: Option<serde_json::Value> = row.get("fields");
    assert!(fields.is_none(), "delete tombstone should have NULL fields");
}

/// An UNDO changeset writes rows with status='UNDO'.
#[tokio::test]
async fn test_apply_undo() {
    require_docker!(sink);

    let new_cs = changeset(200, b"cursor-new", Step::New, vec![upsert("Sell", "sell-1", vec![])]);
    sink.apply(&new_cs).await.expect("apply new");

    let undo_cs = changeset(200, b"cursor-undo", Step::Undo, vec![upsert("Sell", "sell-1", vec![])]);
    sink.apply(&undo_cs).await.expect("apply undo");

    let statuses = status_at_slot(&sink.pool, 200).await;
    assert_eq!(statuses, vec!["NEW", "UNDO"]);
}

/// An Irreversible changeset promotes NEW rows at that slot to FINAL.
#[tokio::test]
async fn test_apply_irreversible() {
    require_docker!(sink);

    let new_cs = changeset(300, b"cursor-new", Step::New, vec![upsert("Swap", "swap-1", vec![])]);
    sink.apply(&new_cs).await.expect("apply new");

    let irrev_cs = changeset(300, b"cursor-final", Step::Irreversible, vec![]);
    sink.apply(&irrev_cs).await.expect("apply irreversible");

    let statuses = status_at_slot(&sink.pool, 300).await;
    assert!(statuses.iter().all(|s| s == "FINAL"), "expected FINAL after Irreversible step");
}

/// Irreversible only promotes rows at the exact slot; other slots are untouched.
#[tokio::test]
async fn test_apply_irreversible_only_at_slot() {
    require_docker!(sink);

    sink.apply(&changeset(400, b"c1", Step::New, vec![upsert("T", "a", vec![])])).await.unwrap();
    sink.apply(&changeset(401, b"c2", Step::New, vec![upsert("T", "b", vec![])])).await.unwrap();

    // Finalize slot 400 only
    sink.apply(&changeset(400, b"c3", Step::Irreversible, vec![])).await.unwrap();

    let s400 = status_at_slot(&sink.pool, 400).await;
    let s401 = status_at_slot(&sink.pool, 401).await;
    assert!(s400.iter().all(|s| s == "FINAL"));
    assert!(s401.iter().all(|s| s == "NEW"));
}

/// Cursor is persisted atomically with the changeset and is correctly resumed.
#[tokio::test]
async fn test_cursor_persistence() {
    require_docker!(sink);

    // No cursor yet on a fresh DB
    let loaded = sink.load_cursor().await.expect("load_cursor");
    assert!(loaded.is_none(), "expected no cursor on fresh DB");

    let cs = changeset(500, b"cursor-abc", Step::New, vec![upsert("T", "x", vec![])]);
    sink.apply(&cs).await.expect("apply");

    let loaded = sink.load_cursor().await.expect("load_cursor after apply");
    assert_eq!(loaded.unwrap().0, b"cursor-abc");
}

/// Subsequent apply calls advance the cursor.
#[tokio::test]
async fn test_cursor_advances() {
    require_docker!(sink);

    sink.apply(&changeset(600, b"cur-1", Step::New, vec![upsert("T", "a", vec![])])).await.unwrap();
    sink.apply(&changeset(601, b"cur-2", Step::New, vec![upsert("T", "b", vec![])])).await.unwrap();

    let cursor = sink.load_cursor().await.unwrap().unwrap();
    assert_eq!(cursor.0, b"cur-2");
}

/// fields_to_json encodes all Value variants correctly.
#[tokio::test]
async fn test_fields_encoding() {
    require_docker!(sink);

    let fields = vec![
        ("str_field", Value::String("hello".into())),
        ("u64_field", Value::U64(42)),
        ("i64_field", Value::I64(-7)),
        ("bool_field", Value::Bool(true)),
        ("null_field", Value::Null),
        ("bytes_field", Value::Bytes(vec![1, 2, 3])),
    ];
    let cs = changeset(700, b"cur", Step::New, vec![upsert("T", "id", fields)]);
    sink.apply(&cs).await.unwrap();

    let row = sqlx::query("SELECT fields FROM entity_changes WHERE entity_id = 'id'")
        .fetch_one(&sink.pool)
        .await
        .unwrap();
    let json: serde_json::Value = row.get("fields");

    assert_eq!(json["str_field"], "hello");
    assert_eq!(json["u64_field"], 42);
    assert_eq!(json["i64_field"], -7);
    assert_eq!(json["bool_field"], true);
    assert!(json["null_field"].is_null());
    // bytes encoded as base58
    assert!(!json["bytes_field"].as_str().unwrap().is_empty());
}

/// Multiple changesets in sequence share the same pool without interference.
#[tokio::test]
async fn test_multiple_changesets() {
    require_docker!(sink);

    for slot in 0..10u64 {
        let cs = changeset(
            slot,
            &slot.to_le_bytes(),
            Step::New,
            vec![upsert("Evt", &format!("id-{slot}"), vec![("slot", Value::U64(slot))])],
        );
        sink.apply(&cs).await.unwrap();
    }

    let count = row_count(&sink.pool).await;
    assert_eq!(count, 10);
}
