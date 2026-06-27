//! Regression tests for the `connected_client` deserialization failure that
//! blanked the admin console: a SCHEMALESS row missing `client_hash` (or other
//! undeclared required fields) used to fail the typed `SurrealValue` read path
//! and terminate the whole store-wide LIVE query. Runs against in-memory
//! SurrealDB so the exact `.take::<Vec<ConnectedClient>>()` path is exercised
//! without a remote client.

use database::schema::{ConnectedClient, RecordId, CONNECTED_CLIENT_TABLE, COMPUTER_TABLE};
use surrealdb::engine::local::{Db, Mem};
use surrealdb::Surreal;

async fn mem_db() -> Surreal<Db> {
    let db = Surreal::new::<Mem>(()).await.expect("in-memory SurrealDB");
    db.use_ns("test").use_db("test").await.expect("use ns/db");
    // Mirror production: SCHEMALESS with no DEFINE FIELD for client_hash /
    // connection_string / connected.
    db.query("DEFINE TABLE connected_client TYPE NORMAL SCHEMALESS")
        .await
        .expect("define schemaless table");
    db
}

#[tokio::test]
async fn schemaless_row_missing_client_hash_still_selects() {
    let db = mem_db().await;
    db.query(
        "CREATE $id SET connection_string = $cs, connected = true, \
         client_kind = 'machine', customer_locked = false",
    )
    .bind(("id", RecordId::new(CONNECTED_CLIENT_TABLE, "DeWittHome:0419a2598")))
    .bind(("cs", "DeWittHome:0419a2598"))
    .await
    .expect("create row without client_hash");

    let mut resp = db
        .query("SELECT * FROM connected_client")
        .await
        .expect("select");
    let rows: Vec<ConnectedClient> = resp
        .take(0)
        .expect("a row missing client_hash must still deserialize (admin-console regression)");
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].client_hash, "");
    assert!(rows[0].connected);
}

#[tokio::test]
async fn schemaless_row_missing_connected_still_selects() {
    let db = mem_db().await;
    db.query(
        "CREATE $id SET client_hash = $h, connection_string = $cs, \
         client_kind = 'machine', customer_locked = false",
    )
    .bind(("id", RecordId::new(CONNECTED_CLIENT_TABLE, "Host:111111111")))
    .bind(("h", "111111111aaaabbbb"))
    .bind(("cs", "Host:111111111"))
    .await
    .expect("create row without connected");

    let mut resp = db
        .query("SELECT * FROM connected_client")
        .await
        .expect("select");
    let rows: Vec<ConnectedClient> = resp
        .take(0)
        .expect("a row missing connected must still deserialize");
    assert_eq!(rows.len(), 1);
    assert!(!rows[0].connected);
}

#[tokio::test]
async fn reported_production_record_selects() {
    let db = mem_db().await;
    // The exact reported row shape: connection_string present, client_hash and
    // computer absent, connected = true.
    db.query(
        "CREATE $id SET assigned_user = $user, client_kind = 'machine', \
         connected = true, connection_string = 'DeWittHome:0419a2598', \
         customer_locked = false, local_ip = '192.168.22.141', tcp_port = 9101",
    )
    .bind(("id", RecordId::new(CONNECTED_CLIENT_TABLE, "DeWittHome:0419a2598")))
    .bind(("user", RecordId::new("user", "jm9a7l3v32gsiccr7pgw")))
    .await
    .expect("create reported production record");

    let mut resp = db
        .query("SELECT * FROM connected_client")
        .await
        .expect("select");
    let rows: Vec<ConnectedClient> = resp
        .take(0)
        .expect("the reported production record must deserialize");
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].client_hash, "");
    assert_eq!(rows[0].connection_string, "DeWittHome:0419a2598");
    assert!(rows[0].computer.is_none());
}

#[tokio::test]
async fn one_bad_row_does_not_poison_the_batch() {
    let db = mem_db().await;
    db.query(
        "CREATE $id SET client_hash = $h, connection_string = $cs, \
         connected = true, client_kind = 'machine', customer_locked = false",
    )
    .bind(("id", RecordId::new(CONNECTED_CLIENT_TABLE, "Good:111111111")))
    .bind(("h", "111111111good"))
    .bind(("cs", "Good:111111111"))
    .await
    .expect("create complete row");
    db.query(
        "CREATE $id SET connection_string = $cs, connected = true, \
         client_kind = 'machine', customer_locked = false",
    )
    .bind(("id", RecordId::new(CONNECTED_CLIENT_TABLE, "Bad:222222222")))
    .bind(("cs", "Bad:222222222"))
    .await
    .expect("create poison row");

    let mut resp = db
        .query("SELECT * FROM connected_client")
        .await
        .expect("select");
    let rows: Vec<ConnectedClient> = resp
        .take(0)
        .expect("the bad row must not drop the good one from the result set");
    assert_eq!(rows.len(), 2, "both rows must deserialize");
}

#[tokio::test]
async fn minimal_row_with_only_connection_string_selects() {
    let db = mem_db().await;
    db.query("CREATE $id SET connection_string = $cs")
        .bind(("id", RecordId::new(CONNECTED_CLIENT_TABLE, "Min:999999999")))
        .bind(("cs", "Min:999999999"))
        .await
        .expect("create minimal row");

    let mut resp = db
        .query("SELECT * FROM connected_client")
        .await
        .expect("select");
    let rows: Vec<ConnectedClient> = resp
        .take(0)
        .expect("a minimal row (only id + connection_string) must deserialize");
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].client_hash, "");
    assert!(!rows[0].connected);
    assert!(!rows[0].customer_locked);
}

#[tokio::test]
async fn identity_upsert_persists_client_hash_and_computer() {
    // Mirrors tcp_listener::upsert_self_identity / the GUI publish UPSERT: the
    // create path must persist both client_hash and computer.
    let db = mem_db().await;
    let id = RecordId::new(CONNECTED_CLIENT_TABLE, "Host:abcdef123");
    let computer = RecordId::new(COMPUTER_TABLE, "Host:abcdef123");
    db.query(
        "UPSERT $id SET client_hash = $h, connection_string = $cs, \
         computer = $computer, connected = $connected, last_update = time::now()",
    )
    .bind(("id", id.clone()))
    .bind(("h", "abcdef123456789"))
    .bind(("cs", "Host:abcdef123"))
    .bind(("computer", computer.clone()))
    .bind(("connected", true))
    .await
    .expect("identity upsert");

    let mut resp = db
        .query("SELECT * FROM connected_client")
        .await
        .expect("select");
    let rows: Vec<ConnectedClient> = resp.take(0).expect("deserialize");
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].client_hash, "abcdef123456789");
    assert_eq!(rows[0].computer, Some(computer));
    assert!(rows[0].connected);
}

#[tokio::test]
async fn defined_field_defaults_supply_identity_on_create() {
    // Validates the proposed production schema hardening: DEFINE FIELD ...
    // DEFAULT fills client_hash / connection_string / connected on a create
    // that omits them.
    let db = Surreal::new::<Mem>(()).await.expect("in-memory SurrealDB");
    db.use_ns("test").use_db("test").await.expect("use ns/db");
    db.query("DEFINE TABLE connected_client TYPE NORMAL SCHEMALESS")
        .await
        .expect("define table");
    db.query("DEFINE FIELD client_hash ON connected_client TYPE string DEFAULT ''")
        .await
        .expect("define client_hash field");
    db.query("DEFINE FIELD connection_string ON connected_client TYPE string DEFAULT ''")
        .await
        .expect("define connection_string field");
    db.query("DEFINE FIELD connected ON connected_client TYPE bool DEFAULT false")
        .await
        .expect("define connected field");

    db.query("CREATE $id SET client_kind = 'machine', customer_locked = false")
        .bind(("id", RecordId::new(CONNECTED_CLIENT_TABLE, "X:123456789")))
        .await
        .expect("create row omitting identity fields");

    let mut resp = db
        .query("SELECT * FROM connected_client")
        .await
        .expect("select");
    let rows: Vec<ConnectedClient> = resp.take(0).expect("deserialize");
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].client_hash, "");
    assert_eq!(rows[0].connection_string, "");
    assert!(!rows[0].connected);
}
