//! Who may read and write the ZeroClaw gateway row, against in-memory SurrealDB with record users signed in.

use database::schema::zeroclaw_gateway::FETCH_GATEWAY_SQL;
use database::schema::ZeroclawGateway;
use serde_json::json;
use surrealdb::engine::local::{Db, Mem};
use surrealdb::opt::auth::Record;
use surrealdb::Surreal;

const SCHEMA: &str = include_str!("../schema/zeroclaw_gateway.surql");
const ROLLOUT: &str = include_str!("../rollouts/20260926150000__zeroclaw_gateway.toml");

const SEED: &str = "
    DEFINE TABLE user TYPE ANY SCHEMALESS PERMISSIONS FOR select WHERE $access = 'user';
    DEFINE ACCESS user ON DATABASE TYPE RECORD SIGNIN (SELECT * FROM user WHERE email = $email);
    DEFINE ACCESS guest ON DATABASE TYPE RECORD SIGNIN (SELECT * FROM guest_user WHERE username = $username);
    CREATE user:root CONTENT { email: 'root@x.com', authorization: 'Root', active: true };
    CREATE user:gone CONTENT { email: 'gone@x.com', authorization: 'Root', active: false };
    CREATE user:tech CONTENT { email: 'tech@x.com', authorization: 'User', active: true };
    CREATE guest_user:guest CONTENT { username: 'guest' };
    CREATE zeroclaw_gateway:shop CONTENT { url: 'https://zc.example', token: 'zc_secret' };
";

/// The SQL of the rollout step named `id`.
fn rollout_step(id: &str) -> &'static str {
    let step = &ROLLOUT[ROLLOUT.find(&format!("id = \"{id}\"")).expect("the step")..];
    let body = &step[step.find("sql = \"\"\"").expect("its sql") + 9..];
    &body[..body.find("\"\"\"").expect("the sql end")]
}

async fn mem_db(schema: &str) -> Surreal<Db> {
    let db = Surreal::new::<Mem>(()).await.expect("in-memory SurrealDB");
    db.use_ns("test").use_db("test").await.expect("use ns/db");
    db.query(schema).await.expect("apply schema").check().expect("schema statements");
    db.query(SEED).await.expect("seed").check().expect("seed statements");
    db
}

async fn session(db: &Surreal<Db>, access: &str, params: serde_json::Value) -> Surreal<Db> {
    let session = db.clone();
    session
        .signin(Record { namespace: "test".into(), database: "test".into(), access: access.into(), params })
        .await
        .expect("record signin");
    session
}

async fn read(session: &Surreal<Db>) -> Vec<ZeroclawGateway> {
    session.query(FETCH_GATEWAY_SQL).await.expect("select").take(0).expect("rows")
}

#[tokio::test]
async fn only_an_active_root_reads_the_gateway() {
    let db = mem_db(SCHEMA).await;
    let root = read(&session(&db, "user", json!({ "email": "root@x.com" })).await).await;
    assert_eq!(root, vec![ZeroclawGateway { url: "https://zc.example".into(), token: "zc_secret".into() }]);
    for email in ["gone@x.com", "tech@x.com"] {
        assert!(read(&session(&db, "user", json!({ "email": email })).await).await.is_empty(), "{email}");
    }
    assert!(read(&session(&db, "guest", json!({ "username": "guest" })).await).await.is_empty());
}

#[tokio::test]
async fn no_record_user_writes_the_gateway() {
    let db = mem_db(SCHEMA).await;
    let root = session(&db, "user", json!({ "email": "root@x.com" })).await;
    for sql in [
        "UPDATE zeroclaw_gateway:shop SET token = 'stolen'",
        "CREATE zeroclaw_gateway:other CONTENT { url: 'https://evil.example', token: 't' }",
        "DELETE zeroclaw_gateway:shop",
    ] {
        root.query(sql).await.expect("statement").check().expect("a denied write is not an error");
    }
    assert_eq!(read(&root).await, vec![ZeroclawGateway { url: "https://zc.example".into(), token: "zc_secret".into() }]);
}

#[tokio::test]
async fn the_rollout_defines_and_removes_the_table() {
    let db = mem_db(rollout_step("define_zeroclaw_gateway")).await;
    assert_eq!(read(&session(&db, "user", json!({ "email": "root@x.com" })).await).await.len(), 1);
    assert!(read(&session(&db, "user", json!({ "email": "tech@x.com" })).await).await.is_empty());
    db.query(rollout_step("remove_zeroclaw_gateway")).await.expect("rollback").check().expect("rollback statements");
    let info: Option<serde_json::Value> = db.query("INFO FOR DB").await.expect("info").take(0).expect("info row");
    let tables = info.and_then(|i| i.get("tables").cloned()).unwrap_or_default();
    assert!(tables.get("zeroclaw_gateway").is_none(), "{tables}");
}
