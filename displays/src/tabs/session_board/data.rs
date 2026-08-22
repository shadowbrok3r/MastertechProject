//! Reads the Claude Code task lists, the captured-item inbox, and session
//! transcripts into the board model.

use std::collections::{BTreeMap, HashMap};
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};

use serde::Deserialize;

const MAX_LINE: usize = 256 * 1024;
const CONTEXT_CHARS: usize = 1400;

pub fn claude_home() -> PathBuf {
    std::env::var("USERPROFILE")
        .map(|p| PathBuf::from(p).join(".claude"))
        .unwrap_or_else(|_| PathBuf::from(".claude"))
}

fn actionable_dir() -> PathBuf {
    claude_home().join("actionable")
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ItemKind {
    Task,
    Suggestion,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ItemStatus {
    Open,
    Archived,
    Dismissed,
    Filed,
}

impl ItemStatus {
    /// Ledger file each closing status is appended to.
    fn ledger(self) -> &'static str {
        match self {
            Self::Open => "reopened.jsonl",
            Self::Archived => "resolved.jsonl",
            Self::Dismissed => "dropped.jsonl",
            Self::Filed => "promoted.jsonl",
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::Open => "open",
            Self::Archived => "archived",
            Self::Dismissed => "dismissed",
            Self::Filed => "filed",
        }
    }
}

#[derive(Clone, Debug)]
pub struct Item {
    pub key: String,
    pub kind: ItemKind,
    pub status: ItemStatus,
    pub subject: String,
    pub detail: String,
    pub ts: String,
    /// Set for items backed by a task-list file, so status writes back.
    pub task_file: Option<PathBuf>,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum GroupKind {
    /// One conversation; the transcript supplies its context.
    Session,
    /// A named shared task list with no single originating conversation.
    Lane,
}

#[derive(Clone, Debug)]
pub struct Group {
    pub id: String,
    pub kind: GroupKind,
    pub title: String,
    pub project: String,
    pub lane: String,
    pub last_active: String,
    pub transcript: Option<PathBuf>,
    pub items: Vec<Item>,
}

impl Group {
    pub fn open_count(&self) -> usize {
        self.items
            .iter()
            .filter(|i| i.status == ItemStatus::Open)
            .count()
    }
}

#[derive(Default)]
pub struct Board {
    pub groups: Vec<Group>,
}

impl Board {
    pub fn open_total(&self) -> usize {
        self.groups.iter().map(Group::open_count).sum()
    }
}

/// First user prompt and last assistant reply of a conversation.
#[derive(Clone, Default)]
pub struct Context {
    pub opening: String,
    pub closing: String,
    pub turns: usize,
}

// ---------------------------------------------------------------- disk shapes

#[derive(Deserialize)]
struct InboxRow {
    key: Option<String>,
    kind: Option<String>,
    subject: Option<String>,
    description: Option<String>,
    ts: Option<String>,
    lane: Option<String>,
    cwd: Option<String>,
    session_id: Option<String>,
    transcript: Option<String>,
    session_title: Option<String>,
}

#[derive(Deserialize)]
struct LedgerRow {
    key: Option<String>,
    ts: Option<String>,
}

#[derive(Deserialize)]
struct TaskRow {
    id: Option<String>,
    subject: Option<String>,
    description: Option<String>,
    status: Option<String>,
}

fn read_jsonl<T: for<'de> Deserialize<'de>>(path: &Path) -> Vec<T> {
    let Ok(text) = fs::read_to_string(path) else {
        return Vec::new();
    };
    text.lines()
        .filter(|l| !l.trim().is_empty() && l.len() < MAX_LINE)
        .filter_map(|l| serde_json::from_str(l).ok())
        .collect()
}

/// Latest status per key across every ledger file; absent means open.
fn ledger_status() -> HashMap<String, (String, ItemStatus)> {
    let dir = actionable_dir();
    let mut out: HashMap<String, (String, ItemStatus)> = HashMap::new();
    for status in [
        ItemStatus::Archived,
        ItemStatus::Dismissed,
        ItemStatus::Filed,
        ItemStatus::Open,
    ] {
        for row in read_jsonl::<LedgerRow>(&dir.join(status.ledger())) {
            let Some(key) = row.key else { continue };
            let ts = row.ts.unwrap_or_default();
            match out.get(&key) {
                Some((prev, _)) if *prev > ts => {}
                _ => {
                    out.insert(key, (ts, status));
                }
            }
        }
    }
    out
}

/// Maps a session id to its transcript by scanning the project store once.
fn transcript_index() -> HashMap<String, PathBuf> {
    let mut out = HashMap::new();
    let root = claude_home().join("projects");
    let Ok(projects) = fs::read_dir(&root) else {
        return out;
    };
    for proj in projects.flatten() {
        let Ok(files) = fs::read_dir(proj.path()) else {
            continue;
        };
        for f in files.flatten() {
            let path = f.path();
            if path.extension().is_some_and(|e| e == "jsonl") {
                if let Some(stem) = path.file_stem().and_then(|s| s.to_str()) {
                    out.insert(stem.to_string(), path);
                }
            }
        }
    }
    out
}

fn is_session_uuid(s: &str) -> bool {
    s.len() == 36
        && s.as_bytes()
            .iter()
            .enumerate()
            .all(|(i, b)| match i {
                8 | 13 | 18 | 23 => *b == b'-',
                _ => b.is_ascii_hexdigit(),
            })
}

fn project_of(cwd: &str) -> String {
    Path::new(cwd)
        .file_name()
        .and_then(|s| s.to_str())
        .unwrap_or("?")
        .to_string()
}

// ------------------------------------------------------------------- loading

pub fn load() -> Board {
    let closed = ledger_status();
    let index = transcript_index();
    let mut groups: BTreeMap<String, Group> = BTreeMap::new();

    for row in read_jsonl::<InboxRow>(&actionable_dir().join("inbox.jsonl")) {
        let Some(key) = row.key else { continue };
        let sid = row.session_id.clone().unwrap_or_else(|| "unknown".into());
        let ts = row.ts.unwrap_or_default();
        let status = closed.get(&key).map_or(ItemStatus::Open, |(_, s)| *s);
        let kind = if row.kind.as_deref() == Some("task") {
            ItemKind::Task
        } else {
            ItemKind::Suggestion
        };

        let g = groups.entry(sid.clone()).or_insert_with(|| Group {
            id: sid.clone(),
            kind: GroupKind::Session,
            title: row
                .session_title
                .clone()
                .unwrap_or_else(|| format!("session {}", &sid[..sid.len().min(8)])),
            project: project_of(row.cwd.as_deref().unwrap_or("")),
            lane: row.lane.clone().unwrap_or_default(),
            last_active: ts.clone(),
            transcript: row
                .transcript
                .as_deref()
                .map(PathBuf::from)
                .filter(|p| p.exists())
                .or_else(|| index.get(&sid).cloned()),
            items: Vec::new(),
        });
        if ts > g.last_active {
            g.last_active = ts.clone();
        }
        if let Some(t) = row.session_title.as_ref() {
            g.title = t.clone();
        }
        g.items.push(Item {
            key,
            kind,
            status,
            subject: row.subject.unwrap_or_default(),
            detail: row.description.unwrap_or_default(),
            ts,
            task_file: None,
        });
    }

    load_task_lists(&mut groups, &closed, &index);

    let mut groups: Vec<Group> = groups.into_values().collect();
    for g in &mut groups {
        g.items.sort_by(|a, b| b.ts.cmp(&a.ts));
    }
    // Most outstanding work first, then most recent.
    groups.sort_by(|a, b| {
        b.open_count()
            .cmp(&a.open_count())
            .then(b.last_active.cmp(&a.last_active))
    });
    Board { groups }
}

/// Folds `~/.claude/tasks/<lane>/<id>.json` into the groups.
fn load_task_lists(
    groups: &mut BTreeMap<String, Group>,
    closed: &HashMap<String, (String, ItemStatus)>,
    index: &HashMap<String, PathBuf>,
) {
    let root = claude_home().join("tasks");
    let Ok(lanes) = fs::read_dir(&root) else {
        return;
    };
    for lane_dir in lanes.flatten() {
        if !lane_dir.path().is_dir() {
            continue;
        }
        let lane = lane_dir.file_name().to_string_lossy().to_string();
        let Ok(files) = fs::read_dir(lane_dir.path()) else {
            continue;
        };
        // A session-local lane is named after the conversation that made it.
        let session_lane = is_session_uuid(&lane);
        let group_id = lane.clone();

        for f in files.flatten() {
            let path = f.path();
            if path.extension().is_none_or(|e| e != "json") {
                continue;
            }
            let Ok(text) = fs::read_to_string(&path) else {
                continue;
            };
            let Ok(task) = serde_json::from_str::<TaskRow>(&text) else {
                continue;
            };
            let raw_status = task.status.unwrap_or_default();
            let tid = task.id.unwrap_or_default();
            let key = format!("tasklist:{lane}:{tid}");
            let ts = fs::metadata(&path)
                .and_then(|m| m.modified())
                .map(|t| {
                    let dt: chrono::DateTime<chrono::Utc> = t.into();
                    dt.format("%Y-%m-%dT%H:%M:%SZ").to_string()
                })
                .unwrap_or_default();

            // A completed task file is closed regardless of the ledger.
            let status = match raw_status.as_str() {
                "completed" => ItemStatus::Archived,
                "cancelled" => ItemStatus::Dismissed,
                _ => closed.get(&key).map_or(ItemStatus::Open, |(_, s)| *s),
            };

            let g = groups.entry(group_id.clone()).or_insert_with(|| Group {
                id: group_id.clone(),
                kind: if session_lane {
                    GroupKind::Session
                } else {
                    GroupKind::Lane
                },
                title: if session_lane {
                    format!("session {}", &lane[..8])
                } else {
                    format!("lane {lane}")
                },
                project: String::new(),
                lane: lane.clone(),
                last_active: ts.clone(),
                transcript: index.get(&lane).cloned(),
                items: Vec::new(),
            });
            if g.lane.is_empty() {
                g.lane = lane.clone();
            }
            if ts > g.last_active {
                g.last_active = ts.clone();
            }
            g.items.push(Item {
                key,
                kind: ItemKind::Task,
                status,
                subject: task.subject.unwrap_or_default(),
                detail: task.description.unwrap_or_default(),
                ts,
                task_file: Some(path),
            });
        }
    }
}

/// Reads a conversation's opening prompt and closing reply.
pub fn load_context(path: &Path) -> Context {
    let Ok(text) = fs::read_to_string(path) else {
        return Context::default();
    };
    let mut ctx = Context::default();
    for line in text.lines() {
        if line.trim().is_empty() || line.len() > MAX_LINE {
            continue;
        }
        let Ok(v) = serde_json::from_str::<serde_json::Value>(line) else {
            continue;
        };
        match v.get("type").and_then(|t| t.as_str()) {
            Some("user") => {
                if ctx.opening.is_empty() {
                    if let Some(t) = message_text(&v) {
                        ctx.opening = clip(&t);
                    }
                }
            }
            Some("assistant") => {
                ctx.turns += 1;
                if let Some(t) = message_text(&v) {
                    if !t.trim().is_empty() {
                        ctx.closing = clip(&t);
                    }
                }
            }
            _ => {}
        }
    }
    ctx
}

/// Concatenated text blocks of a transcript message, skipping tool traffic.
fn message_text(v: &serde_json::Value) -> Option<String> {
    let content = v.get("message")?.get("content")?;
    if let Some(s) = content.as_str() {
        return Some(s.to_string());
    }
    let blocks = content.as_array()?;
    let mut out = String::new();
    for b in blocks {
        if b.get("type").and_then(|t| t.as_str()) == Some("text") {
            if let Some(t) = b.get("text").and_then(|t| t.as_str()) {
                out.push_str(t);
                out.push('\n');
            }
        }
    }
    (!out.trim().is_empty()).then_some(out)
}

fn clip(s: &str) -> String {
    let trimmed = s.trim();
    if trimmed.chars().count() <= CONTEXT_CHARS {
        return trimmed.to_string();
    }
    let cut: String = trimmed.chars().take(CONTEXT_CHARS).collect();
    format!("{cut}…")
}

// ------------------------------------------------------------------- writing

/// Records a status change: appends a ledger event and updates any task file.
pub fn set_status(item: &Item, status: ItemStatus) -> std::io::Result<()> {
    let dir = actionable_dir();
    fs::create_dir_all(&dir)?;
    let ts = chrono::Utc::now().format("%Y-%m-%dT%H:%M:%SZ").to_string();
    let row = serde_json::json!({
        "key": item.key,
        "ts": ts,
        "reason": format!("session board: {}", status.label()),
        "source": "session_board",
    });
    let mut f = fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(dir.join(status.ledger()))?;
    writeln!(f, "{row}")?;

    if let Some(path) = &item.task_file {
        write_task_status(path, status)?;
    }
    Ok(())
}

/// Rewrites a task file's status field, preserving every other key.
fn write_task_status(path: &Path, status: ItemStatus) -> std::io::Result<()> {
    let text = fs::read_to_string(path)?;
    let mut v: serde_json::Value = serde_json::from_str(&text)
        .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))?;
    let next = match status {
        ItemStatus::Archived | ItemStatus::Filed => "completed",
        ItemStatus::Dismissed => "cancelled",
        ItemStatus::Open => "pending",
    };
    v["status"] = serde_json::Value::String(next.to_string());
    fs::write(path, serde_json::to_string_pretty(&v)?)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn is_session_uuid_matches_lane_names() {
        assert!(is_session_uuid("587cc283-ed4f-4d99-975e-9c5b41b868ea"));
        assert!(!is_session_uuid("mtech-dev"));
        assert!(!is_session_uuid("587cc283ed4f4d99975e9c5b41b868ea"));
    }

    /// Prints the live board so the loader can be eyeballed against real data.
    #[test]
    fn loads_local_board() {
        let board = load();
        println!(
            "groups={} open={}",
            board.groups.len(),
            board.open_total()
        );
        for g in board.groups.iter().take(8) {
            println!(
                "[{}] {} | proj={} lane={} open={}/{} transcript={}",
                match g.kind {
                    GroupKind::Session => "session",
                    GroupKind::Lane => "lane",
                },
                g.title,
                g.project,
                g.lane,
                g.open_count(),
                g.items.len(),
                g.transcript.is_some()
            );
            for i in g.items.iter().filter(|i| i.status == ItemStatus::Open).take(3) {
                println!("      - {} [{}]", i.subject, i.status.label());
            }
        }
    }
}
