//! `user-admin`: staff account sync and test-user creation over a root connection from the repo `.env`.

mod apply;
mod plan;
mod report;
mod source;
#[cfg(test)]
mod test_db;
mod test_user;

use std::fs::{self, File, OpenOptions};
use std::io::Write as _;
use std::path::{Path, PathBuf};
use std::time::Duration;

use anyhow::{Context, Result, anyhow, bail};
use clap::{Args, Parser, Subcommand};
use database::schema::{employee_directory, normalize_email};
use surrealdb::Surreal;
use surrealdb::engine::remote::ws::{Client, Ws, Wss};
use surrealdb::opt::auth::Root;

use crate::apply::{Statement, SyncRecord, Target, Write, sets_war};
use crate::plan::{Reassign, SyncOptions};

const ENV_PATH: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../.env");
const OUT_DIR: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/out");
const CONNECT_TIMEOUT: Duration = Duration::from_secs(20);
const RECORD_FORMAT: u32 = 1;
const WAR_WARNING: &str = "WARNING: --allow-war writes store 'WAR'. Every build without the WAR \
variant fails every login once a WAR row exists. Use it only after every client carries WAR.";

#[derive(Debug, Parser)]
#[command(
    name = "user-admin",
    version,
    about = "Staff account maintenance for the Mastertech SurrealDB (root connection)."
)]
struct Cli {
    /// Connect to DB_URL_LOCAL over ws instead of DB_URL_DEV over wss.
    #[arg(long, global = true)]
    local: bool,

    #[command(subcommand)]
    command: Command,
}

#[derive(Debug, Subcommand)]
enum Command {
    /// Compare users with the employee directory; a dry run unless --apply.
    Sync(SyncArgs),
    /// Create a non-employee test account; the password is prompted twice.
    CreateTestUser(TestUserArgs),
}

#[derive(Debug, Args)]
struct SyncArgs {
    /// Write the plan in one guarded transaction after saving out/sync_<utc>.json.
    #[arg(long)]
    apply: bool,

    /// Include moves to WAR (held otherwise).
    #[arg(long)]
    allow_war: bool,

    /// Deactivate this active user missing from the directory.
    #[arg(long, value_name = "EMAIL", value_parser = parse_email)]
    deactivate_missing: Vec<String>,

    /// Leave this user untouched.
    #[arg(long, value_name = "EMAIL", value_parser = parse_email)]
    skip: Vec<String>,

    /// Move FROM's open tasks to TO when FROM is deactivated.
    #[arg(long, value_name = "FROM=TO", value_parser = parse_reassign)]
    reassign: Vec<Reassign>,

    /// Deactivate this user and leave their open tasks on the inactive account.
    #[arg(long, value_name = "EMAIL", value_parser = parse_email)]
    orphan_tasks: Vec<String>,

    /// Deactivation limit (default: 20% of active users).
    #[arg(long, value_name = "N")]
    max_deactivations: Option<usize>,

    /// Plan writes for this user from its id_prestashop record even when that record's email differs.
    #[arg(long, value_name = "EMAIL", value_parser = parse_email)]
    trust_id: Vec<String>,

    /// Print the plan as JSON instead of the report.
    #[arg(long)]
    json: bool,

    /// Undo a committed sync from its out/sync_<utc>.json; a preview unless --apply.
    #[arg(
        long,
        value_name = "FILE",
        conflicts_with_all = ["deactivate_missing", "skip", "reassign", "orphan_tasks", "max_deactivations", "trust_id", "json"]
    )]
    revert: Option<PathBuf>,
}

impl SyncArgs {
    fn options(&self) -> SyncOptions {
        SyncOptions {
            allow_war: self.allow_war,
            deactivate_missing: self.deactivate_missing.clone(),
            skip: self.skip.clone(),
            reassign: self.reassign.clone(),
            orphan_tasks: self.orphan_tasks.clone(),
            max_deactivations: self.max_deactivations,
            trust_id: self.trust_id.clone(),
        }
    }
}

#[derive(Debug, Args)]
struct TestUserArgs {
    /// Full email address of the new account.
    #[arg(long)]
    email: String,

    /// Display name.
    #[arg(long)]
    name: String,

    /// Retail store code.
    #[arg(long, default_value = "RIV")]
    store: String,

    /// User, Manager or Warehouse.
    #[arg(long, default_value = "User")]
    authorization: String,
}

/// A full, normalized email address.
fn parse_email(value: &str) -> Result<String, String> {
    if !value.contains('@') {
        return Err(format!("{value:?} is not a full email address"));
    }
    normalize_email(value).ok_or_else(|| format!("{value:?} is not a valid email address"))
}

fn parse_reassign(value: &str) -> Result<Reassign, String> {
    let (from, to) = value
        .split_once('=')
        .ok_or_else(|| format!("{value:?} is not FROM=TO"))?;
    Ok(Reassign {
        from: parse_email(from)?,
        to: parse_email(to)?,
    })
}

#[tokio::main]
async fn main() -> Result<()> {
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("info"))
        .filter_module("surrealdb", log::LevelFilter::Warn)
        .filter_module("surrealdb_core", log::LevelFilter::Warn)
        .filter_module("tungstenite", log::LevelFilter::Warn)
        .filter_module("tokio_tungstenite", log::LevelFilter::Warn)
        .target(env_logger::Target::Stderr)
        .try_init()
        .ok();

    let cli = Cli::parse();
    match cli.command {
        Command::Sync(args) => match args.revert.clone() {
            Some(file) => revert(cli.local, &file, &args).await,
            None => sync(cli.local, &args).await,
        },
        Command::CreateTestUser(args) => create_test_user(cli.local, &args).await,
    }
}

/// `DB_ROOT_USER` / `DB_ROOT_PASS` from the repo `.env`, falling back to the environment.
fn root_credentials() -> Result<(String, String)> {
    let mut user = None;
    let mut pass = None;
    if let Ok(items) = dotenvy::from_path_iter(ENV_PATH) {
        for item in items {
            let (key, value) = item.map_err(|_| anyhow!("could not parse {ENV_PATH}"))?;
            match key.as_str() {
                "DB_ROOT_USER" => user = Some(value),
                "DB_ROOT_PASS" => pass = Some(value),
                _ => {}
            }
        }
    }
    let pick = |found: Option<String>, key: &str| {
        found
            .filter(|value| !value.is_empty())
            .or_else(|| std::env::var(key).ok().filter(|value| !value.is_empty()))
    };
    match (pick(user, "DB_ROOT_USER"), pick(pass, "DB_ROOT_PASS")) {
        (Some(user), Some(pass)) => Ok((user, pass)),
        _ => bail!("DB_ROOT_USER and DB_ROOT_PASS must be set in {ENV_PATH} or the environment"),
    }
}

/// Root session on DB_URL_DEV (wss) or DB_URL_LOCAL (ws), scoped to NS/DB.
async fn connect(local: bool) -> Result<(Surreal<Client>, Target)> {
    let _ = rustls::crypto::aws_lc_rs::default_provider().install_default();
    let (username, password) = root_credentials()?;
    let url = if local {
        database::DB_URL_LOCAL
    } else {
        database::DB_URL_DEV
    };
    let target = Target {
        url: url.to_string(),
        local,
        ns: database::NS.to_string(),
        db: database::DB.to_string(),
    };
    let session = async {
        let db = if local {
            Surreal::new::<Ws>(url).await?
        } else {
            Surreal::new::<Wss>(url).await?
        };
        db.signin(Root { username, password }).await?;
        db.use_ns(database::NS).use_db(database::DB).await?;
        Ok::<_, surrealdb::Error>(db)
    };
    let db = tokio::time::timeout(CONNECT_TIMEOUT, session)
        .await
        .with_context(|| format!("timed out connecting to {target}"))?
        .with_context(|| format!("root connection to {target} failed"))?;
    eprintln!("user-admin: connected to {target}");
    Ok((db, target))
}

async fn sync(local: bool, args: &SyncArgs) -> Result<()> {
    if args.allow_war {
        eprintln!("{WAR_WARNING}");
    }
    let (db, target) = connect(local).await?;
    let users = source::read_users(&db).await?;
    eprintln!("user-admin sync: looking up {} users", users.len());
    let entries = source::look_up(&employee_directory(), users, &args.skip).await?;
    let active_ids = entries
        .iter()
        .filter(|entry| entry.user.active)
        .map(|entry| entry.user.id.clone())
        .collect();
    let open_tasks = source::read_open_tasks(&db, active_ids).await?;
    let plan = plan::plan(&entries, &open_tasks, &args.options());

    if args.json {
        println!("{}", serde_json::to_string_pretty(&plan)?);
    } else {
        print!("{}", report::render(&plan, args.apply));
    }
    if !args.apply {
        return Ok(());
    }
    if !plan.refused.is_empty() {
        bail!("--apply refused: resolve the REFUSED items first; nothing was written");
    }
    let writes = apply::writes(&plan);
    if writes.is_empty() {
        return Ok(());
    }
    if !args.allow_war && sets_war(writes.iter().map(Write::forward).collect::<Vec<_>>().iter()) {
        bail!("the plan writes store 'WAR' without --allow-war; nothing was written");
    }

    let created = chrono::Utc::now();
    let mut record = SyncRecord {
        format: RECORD_FORMAT,
        created_at: utc_stamp(created),
        committed_at: None,
        reverted_at: None,
        target,
        plan,
        writes,
    };
    let path = write_record(
        Path::new(OUT_DIR),
        &record,
        &created.format("%Y%m%dT%H%M%SZ").to_string(),
    )?;
    eprintln!("user-admin sync: saved {}", path.display());

    if let Err(err) = source::run(&db, Statement::apply(&record.writes)).await {
        if err.downcast_ref::<source::Aborted>().is_some() {
            let aborted = path.with_extension("aborted.json");
            match fs::rename(&path, &aborted) {
                Ok(()) => eprintln!(
                    "user-admin sync: renamed the record to {}",
                    aborted.display()
                ),
                Err(rename) => eprintln!(
                    "user-admin sync: could not rename {}: {rename}",
                    path.display()
                ),
            }
        } else {
            eprintln!(
                "user-admin sync: outcome unknown; {} stays unmarked and --revert refuses it",
                path.display()
            );
        }
        return Err(err);
    }
    let count = record.writes.len();
    eprintln!("user-admin sync: committed {count} write(s)");
    record.committed_at = Some(utc_stamp(chrono::Utc::now()));
    replace_record(&path, &record).with_context(|| {
        format!(
            "the {count} write(s) are committed, but marking {} committed failed; --revert refuses it until committed_at is set",
            path.display()
        )
    })?;
    eprintln!(
        "user-admin sync: undo with: user-admin sync --revert {} --apply",
        path.display()
    );
    Ok(())
}

fn utc_stamp(at: chrono::DateTime<chrono::Utc>) -> String {
    at.to_rfc3339_opts(chrono::SecondsFormat::Secs, true)
}

fn write_json(file: &mut File, record: &SyncRecord) -> Result<()> {
    serde_json::to_writer_pretty(&mut *file, record)?;
    file.write_all(b"\n")?;
    file.sync_all()?;
    Ok(())
}

/// Saves `record` to `<dir>/sync_<stamp>.json` without overwriting an existing file.
fn write_record(dir: &Path, record: &SyncRecord, stamp: &str) -> Result<PathBuf> {
    fs::create_dir_all(dir).with_context(|| format!("create {}", dir.display()))?;
    let path = dir.join(format!("sync_{stamp}.json"));
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&path)
        .with_context(|| format!("create {}", path.display()))?;
    write_json(&mut file, record)?;
    Ok(path)
}

/// Rewrites `path` with `record` through a temporary file in the same directory.
fn replace_record(path: &Path, record: &SyncRecord) -> Result<()> {
    let tmp = path.with_extension("json.tmp");
    let mut file = File::create(&tmp).with_context(|| format!("create {}", tmp.display()))?;
    write_json(&mut file, record)?;
    drop(file);
    fs::rename(&tmp, path).with_context(|| format!("replace {}", path.display()))
}

/// Refuses aborted, uncommitted, already reverted or foreign-format records.
fn check_revertable(file: &Path, record: &SyncRecord) -> Result<()> {
    let name = file
        .file_name()
        .map(|name| name.to_string_lossy().to_lowercase())
        .unwrap_or_default();
    if name.ends_with(".aborted.json") {
        bail!(
            "{} is the record of an aborted sync; nothing was written, so there is nothing to revert",
            file.display()
        );
    }
    if record.format != RECORD_FORMAT {
        bail!(
            "{} has record format {}, expected {RECORD_FORMAT}",
            file.display(),
            record.format
        );
    }
    let Some(committed) = &record.committed_at else {
        bail!(
            "{} is not marked committed: its sync aborted or its outcome is unknown; check the rows before changing anything",
            file.display()
        );
    };
    if let Some(reverted) = &record.reverted_at {
        bail!(
            "{} (committed {committed}) was already reverted at {reverted}",
            file.display()
        );
    }
    Ok(())
}

async fn revert(local: bool, file: &Path, args: &SyncArgs) -> Result<()> {
    let text = fs::read_to_string(file).with_context(|| format!("read {}", file.display()))?;
    let mut record: SyncRecord =
        serde_json::from_str(&text).with_context(|| format!("parse {}", file.display()))?;
    check_revertable(file, &record)?;
    let inverse: Vec<_> = record.writes.iter().map(Write::inverse).collect();
    if sets_war(&inverse) {
        if !args.allow_war {
            bail!("this revert restores store 'WAR'; pass --allow-war to write it");
        }
        eprintln!("{WAR_WARNING}");
    }
    let (db, target) = connect(local).await?;
    if target != record.target {
        bail!(
            "{} was written against {}, not {target}",
            file.display(),
            record.target
        );
    }
    print!("{}", report::render_revert(&record, args.apply));
    if !args.apply || record.writes.is_empty() {
        return Ok(());
    }
    source::run(&db, Statement::revert(&record.writes)).await?;
    let count = record.writes.len();
    eprintln!("user-admin sync --revert: committed {count} write(s)");
    record.reverted_at = Some(utc_stamp(chrono::Utc::now()));
    replace_record(file, &record).with_context(|| {
        format!(
            "the revert of {count} write(s) is committed, but marking {} reverted failed",
            file.display()
        )
    })
}

async fn create_test_user(local: bool, args: &TestUserArgs) -> Result<()> {
    let user = test_user::validate(&args.email, &args.name, &args.store, &args.authorization)?;
    let (db, _) = connect(local).await?;
    if test_user::email_exists(&db, &user.email).await? {
        return Err(test_user::TestUserError::EmailExists(user.email).into());
    }
    let legacy = test_user::legacy_fields(&test_user::user_fields(&db).await?);
    eprintln!(
        "Creating {} ({}, {}, {})",
        user.email,
        user.name,
        user.store.as_str(),
        user.authorization.as_str()
    );
    let password = rpassword::prompt_password("Password: ")?;
    let repeat = rpassword::prompt_password("Repeat password: ")?;
    test_user::check_passwords(&password, &repeat)?;
    drop(repeat);
    let created = test_user::create(&db, &user, password, legacy).await?;
    println!(
        "Created {} {} store={} authorization={} active={}",
        plan::rid(&created.id),
        created.email,
        created.store,
        created.authorization,
        created.active
    );
    Ok(())
}

#[cfg(test)]
mod tests {
    use clap::CommandFactory;

    use super::*;

    #[test]
    fn the_cli_definition_is_consistent() {
        Cli::command().debug_assert();
    }

    #[test]
    fn email_flags_require_a_full_address() {
        assert_eq!(
            parse_email(" Jane@PCLaptops.com "),
            Ok("jane@pclaptops.com".into())
        );
        assert!(parse_email("jane").is_err());
        assert!(parse_email("a b@pclaptops.com").is_err());
    }

    #[test]
    fn reassign_parses_both_sides() {
        assert_eq!(
            parse_reassign("Jane@pclaptops.com=bob@XIDAX.com"),
            Ok(Reassign {
                from: "jane@pclaptops.com".into(),
                to: "bob@xidax.com".into()
            })
        );
        assert!(parse_reassign("jane@pclaptops.com").is_err());
        assert!(parse_reassign("jane=bob@pclaptops.com").is_err());
    }

    #[test]
    fn sync_flags_parse_and_revert_conflicts_with_planning_flags() {
        let cli = Cli::try_parse_from([
            "user-admin",
            "--local",
            "sync",
            "--apply",
            "--skip",
            "a@pclaptops.com",
            "--skip",
            "b@pclaptops.com",
            "--reassign",
            "c@pclaptops.com=d@pclaptops.com",
            "--max-deactivations",
            "4",
        ])
        .expect("parses");
        assert!(cli.local);
        let Command::Sync(args) = cli.command else {
            panic!("sync");
        };
        assert!(args.apply && !args.allow_war);
        assert_eq!(args.skip, vec!["a@pclaptops.com", "b@pclaptops.com"]);
        assert_eq!(args.max_deactivations, Some(4));

        assert!(
            Cli::try_parse_from([
                "user-admin",
                "sync",
                "--revert",
                "f.json",
                "--skip",
                "a@pclaptops.com"
            ])
            .is_err()
        );
        assert!(
            Cli::try_parse_from(["user-admin", "sync", "--revert", "f.json", "--apply"]).is_ok()
        );
    }

    fn record(committed: bool) -> SyncRecord {
        let entries = crate::plan::tests::staff();
        let plan = crate::plan::plan(&entries, &Default::default(), &SyncOptions::default());
        SyncRecord {
            format: RECORD_FORMAT,
            created_at: "2026-09-26T10:00:00Z".into(),
            committed_at: committed.then(|| "2026-09-26T10:00:01Z".into()),
            reverted_at: None,
            target: Target {
                url: "127.0.0.1:8000".into(),
                local: true,
                ns: "ns".into(),
                db: "db".into(),
            },
            plan,
            writes: Vec::new(),
        }
    }

    #[test]
    fn only_committed_unreverted_records_can_be_reverted() {
        let file = Path::new("out/sync_20260926T100000Z.json");
        assert!(check_revertable(file, &record(true)).is_ok());

        let err = check_revertable(file, &record(false)).expect_err("uncommitted");
        assert!(err.to_string().contains("not marked committed"), "{err:#}");

        let aborted = Path::new("out/sync_20260926T100000Z.aborted.json");
        let err = check_revertable(aborted, &record(true)).expect_err("aborted");
        assert!(err.to_string().contains("aborted sync"), "{err:#}");

        let mut reverted = record(true);
        reverted.reverted_at = Some("2026-09-27T08:00:00Z".into());
        let err = check_revertable(file, &reverted).expect_err("reverted");
        assert!(err.to_string().contains("already reverted"), "{err:#}");

        let mut old = record(true);
        old.format = RECORD_FORMAT + 1;
        assert!(check_revertable(file, &old).is_err());
    }

    #[test]
    fn a_record_without_commit_fields_reads_as_uncommitted() {
        let mut json = serde_json::to_value(record(true)).expect("serialize");
        let fields = json.as_object_mut().expect("object");
        fields.remove("committed_at");
        fields.remove("reverted_at");
        let back: SyncRecord = serde_json::from_value(json).expect("deserialize");
        assert_eq!((back.committed_at, back.reverted_at), (None, None));
    }

    #[test]
    fn a_saved_record_is_marked_committed_in_place() {
        let dir = std::env::temp_dir().join(format!("user-admin-test-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        let mut rec = record(false);
        let path = write_record(&dir, &rec, "20260926T100000Z").expect("write");
        assert!(
            write_record(&dir, &rec, "20260926T100000Z").is_err(),
            "no overwrite"
        );

        rec.committed_at = Some("2026-09-26T10:00:01Z".into());
        replace_record(&path, &rec).expect("replace");
        let back: SyncRecord =
            serde_json::from_str(&fs::read_to_string(&path).expect("read")).expect("parse");
        assert_eq!(back, rec);
        let names: Vec<String> = fs::read_dir(&dir)
            .expect("list")
            .map(|entry| {
                entry
                    .expect("entry")
                    .file_name()
                    .to_string_lossy()
                    .into_owned()
            })
            .collect();
        assert_eq!(names, vec!["sync_20260926T100000Z.json"]);
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn create_test_user_has_no_password_flag() {
        assert!(
            Cli::try_parse_from([
                "user-admin",
                "create-test-user",
                "--email",
                "a@pclaptops.com",
                "--name",
                "A",
                "--password",
                "x"
            ])
            .is_err()
        );
    }
}
