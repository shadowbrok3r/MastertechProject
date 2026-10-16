//! Similarity search over `diagnostic_entry` against in-memory SurrealDB with the real schema.

use database::schema::{
    CASE_SESSIONS_SQL, CaseSession, DIAGNOSTIC_ENTRY_TABLE, DIAGNOSTIC_SESSION_TABLE,
    DiagnosticCategory, KnnSearch, RecordId, SimilarEntry, SimilarFilter, nearest_entries_sql,
};
use surrealdb::Surreal;
use surrealdb::engine::local::{Db, Mem};

/// 768-dim vector holding `x` and `y` in its first two dimensions and zeros elsewhere.
fn vector(x: f32, y: f32) -> Vec<f32> {
    let mut v = vec![0.0f32; 768];
    v[0] = x;
    v[1] = y;
    v
}

fn session(key: &str) -> RecordId {
    RecordId::new(DIAGNOSTIC_SESSION_TABLE, key)
}

/// Entries at cosine distance 0, 0.019, 0.106, 0.293 and 1 from `vector(1, 0)`.
async fn mem_db() -> Surreal<Db> {
    let db = Surreal::new::<Mem>(()).await.expect("in-memory SurrealDB");
    db.use_ns("test").use_db("test").await.expect("use ns/db");
    db.query(include_str!("../schema/diagnostic_entry.surql"))
        .await
        .expect("apply schema")
        .check()
        .expect("schema statements");
    let rows = [
        ("a1", "a", "finding", false, (1.0, 0.0), "x".repeat(500)),
        ("a2", "a", "note", false, (1.0, 0.2), "short".to_string()),
        (
            "b1",
            "b",
            "recommendation",
            true,
            (1.0, 0.5),
            "lesson".to_string(),
        ),
        (
            "d1",
            "d",
            "finding",
            false,
            (1.0, 1.0),
            "orphan".to_string(),
        ),
        ("c1", "c", "finding", false, (0.0, 1.0), "far".to_string()),
    ];
    for (key, sess, category, lesson, (x, y), detail) in rows {
        db.query(
            "CREATE $id CONTENT { session_ref: $session_ref, category: $category, title: $title, \
             detail: $detail, data: $data, plugins_used: [], embedding: $embedding }",
        )
        .bind(("id", RecordId::new(DIAGNOSTIC_ENTRY_TABLE, key)))
        .bind(("session_ref", session(sess)))
        .bind(("category", category.to_string()))
        .bind(("title", key.to_string()))
        .bind(("detail", detail))
        .bind((
            "data",
            lesson.then(|| serde_json::json!({ "flywheel_lesson": true })),
        ))
        .bind(("embedding", vector(x, y)))
        .await
        .expect("create entry")
        .check()
        .expect("entry row");
    }
    db.query(
        "CREATE diagnostic_session:a CONTENT { hostname: 'PC-A', status: 'resolved', summary: 'Replaced the PSU' };
         CREATE diagnostic_session:b CONTENT { hostname: 'PC-B', status: 'escalated' };
         CREATE diagnostic_session:c CONTENT { hostname: 'PC-C', status: 'open' };",
    )
    .await
    .expect("create sessions")
    .check()
    .expect("session rows");
    db
}

async fn nearest(
    db: &Surreal<Db>,
    filter: &SimilarFilter,
    k: usize,
    search: KnnSearch,
) -> Vec<SimilarEntry> {
    db.query(nearest_entries_sql(filter, k, search))
        .bind(("vec", vector(1.0, 0.0)))
        .bind(("category", filter.category))
        .bind(("exclude", filter.exclude_session.clone()))
        .await
        .expect("knn query")
        .take(0)
        .expect("similar entries")
}

fn titles(rows: &[SimilarEntry]) -> Vec<&str> {
    rows.iter().map(|r| r.title.as_str()).collect()
}

#[tokio::test]
async fn index_and_exact_searches_rank_entries_by_cosine_distance() {
    let db = mem_db().await;
    for search in [KnnSearch::Index, KnnSearch::Exact] {
        let rows = nearest(&db, &SimilarFilter::default(), 10, search).await;
        assert_eq!(titles(&rows), ["a1", "a2", "b1", "d1", "c1"], "{search:?}");
        assert!(rows[0].dist.abs() < 1e-4, "{search:?}: {}", rows[0].dist);
        assert!(
            (rows[3].dist - 0.2929).abs() < 1e-3,
            "{search:?}: {}",
            rows[3].dist
        );
        assert_eq!(rows[0].detail.chars().count(), 400, "{search:?}");
        assert_eq!(rows[1].detail, "short", "{search:?}");
        assert_eq!(
            rows[2].category,
            DiagnosticCategory::Recommendation,
            "{search:?}"
        );
    }
}

#[tokio::test]
async fn a_search_returns_at_most_k_entries() {
    let db = mem_db().await;
    for search in [KnnSearch::Index, KnnSearch::Exact] {
        let rows = nearest(&db, &SimilarFilter::default(), 2, search).await;
        assert_eq!(titles(&rows), ["a1", "a2"], "{search:?}");
    }
}

#[tokio::test]
async fn filters_narrow_both_searches() {
    let db = mem_db().await;
    let findings = SimilarFilter {
        category: Some(DiagnosticCategory::Finding),
        ..Default::default()
    };
    let lessons = SimilarFilter {
        lessons_only: true,
        ..Default::default()
    };
    let not_a = SimilarFilter {
        exclude_session: Some(session("a")),
        ..Default::default()
    };
    for search in [KnnSearch::Index, KnnSearch::Exact] {
        assert_eq!(
            titles(&nearest(&db, &findings, 10, search).await),
            ["a1", "d1", "c1"],
            "{search:?}"
        );
        assert_eq!(
            titles(&nearest(&db, &lessons, 10, search).await),
            ["b1"],
            "{search:?}"
        );
        assert_eq!(
            titles(&nearest(&db, &not_a, 10, search).await),
            ["b1", "d1", "c1"],
            "{search:?}"
        );
    }
}

#[tokio::test]
async fn case_sessions_skip_sessions_that_no_longer_exist() {
    let db = mem_db().await;
    let rows: Vec<CaseSession> = db
        .query(CASE_SESSIONS_SQL)
        .bind(("ids", vec![session("a"), session("b"), session("d")]))
        .await
        .expect("select sessions")
        .take(0)
        .expect("session rows");
    let mut hosts: Vec<String> = rows.iter().filter_map(|r| r.hostname.clone()).collect();
    hosts.sort();
    assert_eq!(hosts, ["PC-A", "PC-B"]);
    let a = rows
        .iter()
        .find(|r| r.id == session("a"))
        .expect("session a");
    assert_eq!(a.summary.as_deref(), Some("Replaced the PSU"));
    assert_eq!(a.current_theory, None);
}
