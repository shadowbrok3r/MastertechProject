//! Fleet crash-signature intelligence.
//!
//! Normalizes WinDbg `!analyze -v` results (via the `com.mastertech.dump-decode`
//! plugin) into `(bugcheck_code, module)` signatures upserted in place, appends a
//! `crash_sighting` per decoded dump, and looks up prior `crash_verdict` rows so a
//! repeat crash surfaces the known diagnosis/fix immediately.

use serde::{Deserialize, Serialize};

use crate::db;

use super::{Datetime, RecordId, SurrealValue};

/// One normalized crash class: a (bugcheck, module) pair seen across the fleet.
#[derive(Debug, Clone, Serialize, Deserialize, SurrealValue)]
pub struct CrashSignature {
    pub id: RecordId,
    pub bugcheck_code: String,
    #[serde(default)]
    pub bugcheck_name: String,
    pub module: String,
    #[serde(default)]
    pub offsets: Vec<String>,
    #[serde(default)]
    pub module_versions: Vec<String>,
    #[serde(default)]
    pub failure_buckets: Vec<String>,
    #[serde(default)]
    pub machines: Vec<String>,
    #[serde(default)]
    pub sighting_count: u32,
    pub first_seen: Datetime,
    pub last_seen: Datetime,
    #[serde(default)]
    pub latest_verdict: Option<RecordId>,
    #[serde(default)]
    pub tags: Vec<String>,
}

/// One decoded dump occurrence tied to a signature.
#[derive(Debug, Clone, Serialize, Deserialize, SurrealValue)]
pub struct CrashSighting {
    pub id: RecordId,
    pub signature: RecordId,
    #[serde(default)]
    pub connection_string: Option<String>,
    #[serde(default)]
    pub computer: Option<RecordId>,
    #[serde(default)]
    pub session_ref: Option<RecordId>,
    #[serde(default)]
    pub task_ref: Option<RecordId>,
    #[serde(default)]
    pub dump_name: Option<String>,
    #[serde(default)]
    pub dump_kind: String,
    #[serde(default)]
    pub dump_time: Option<String>,
    #[serde(default)]
    pub offset: Option<String>,
    #[serde(default)]
    pub module_version: Option<String>,
    #[serde(default)]
    pub failure_bucket: Option<String>,
    #[serde(default)]
    pub process_name: Option<String>,
    #[serde(default)]
    pub caused_by: Option<String>,
    #[serde(default)]
    pub raw_excerpt: String,
    /// Normalized (`module_stem`) names of every module loaded at crash time.
    /// The queryable asset: "which machines had driver X loaded when they
    /// crashed with bugcheck Y". Empty when the dump carried no module list.
    #[serde(default)]
    pub loaded_modules: Vec<String>,
    /// Structured per-dump forensic detail (decoded params, RIP module,
    /// uptime, driver base/size list). Free-form so it can grow without a
    /// migration on this SCHEMALESS table.
    #[serde(default)]
    pub triage: Option<serde_json::Value>,
    pub created_at: Datetime,
}

/// Diagnosis + fix recorded against a signature.
#[derive(Debug, Clone, Serialize, Deserialize, SurrealValue)]
pub struct CrashVerdict {
    pub id: RecordId,
    pub signature: RecordId,
    pub verdict: String,
    #[serde(default)]
    pub fix: String,
    #[serde(default)]
    pub confidence: String,
    #[serde(default)]
    pub author: String,
    #[serde(default)]
    pub source: String,
    #[serde(default)]
    pub task_ref: Option<RecordId>,
    pub created_at: Datetime,
}

/// Crash fields extracted from one decoded dump, pre-normalization.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
pub struct ParsedCrash {
    pub bugcheck_code: String,
    pub bugcheck_name: String,
    pub module: String,
    pub offset: Option<String>,
    pub process_name: Option<String>,
    pub failure_bucket: Option<String>,
    pub caused_by: Option<String>,
    /// Version marker for the blamed module (triage path: PE link date).
    #[serde(default)]
    pub module_version: Option<String>,
    pub dump_name: Option<String>,
    pub dump_time: Option<String>,
    pub raw_excerpt: String,
    /// Normalized loaded-module names (crash-time). Persisted to the sighting
    /// for fleet co-occurrence queries. Default empty for parsers that don't
    /// carry a module list (e.g. WinDbg text output).
    #[serde(default)]
    pub loaded_modules: Vec<String>,
    /// Structured forensic blob persisted verbatim onto the sighting.
    #[serde(default)]
    pub triage: Option<serde_json::Value>,
}

/// Machine/task context attached to ingested sightings.
#[derive(Debug, Clone, Default)]
pub struct SightingContext {
    pub connection_string: Option<String>,
    pub computer: Option<RecordId>,
    pub session_ref: Option<RecordId>,
    pub task_ref: Option<RecordId>,
    pub dump_kind: String,
}

/// Ingest outcome: the merged signature plus whatever the fleet already knew.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CrashIngest {
    pub signature: CrashSignature,
    pub sighting_id: RecordId,
    pub previously_seen: bool,
    pub prior_sighting_count: u32,
    pub prior_machine_count: u32,
    pub verdicts: Vec<CrashVerdict>,
}

/// Outcome of a session-link reconcile sweep.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ReconcileReport {
    pub sightings_claimed: usize,
    pub sightings_task_linked: usize,
    pub snapshots_claimed: usize,
    pub sightings_enriched: usize,
}

impl ReconcileReport {
    pub fn total(&self) -> usize {
        self.sightings_claimed
            + self.sightings_task_linked
            + self.snapshots_claimed
            + self.sightings_enriched
    }

    /// One-line account of what the sweep changed, covering every counter so
    /// an enrichment-only sweep isn't reported as "0 sightings, 0 snapshots".
    pub fn summary(&self) -> String {
        format!(
            "{} sighting(s) claimed, {} task link(s) propagated, {} snapshot(s) claimed, {} sibling field(s) enriched",
            self.sightings_claimed,
            self.sightings_task_linked,
            self.snapshots_claimed,
            self.sightings_enriched
        )
    }
}

/// `crash_signature:<0xNNN_module>` so repeat crashes upsert in place.
pub fn crash_signature_record_id(bugcheck_code: &str, module: &str) -> RecordId {
    let key = format!(
        "{}_{}",
        bugcheck_code.trim().to_ascii_lowercase(),
        module.trim().to_ascii_lowercase()
    );
    RecordId::new(super::CRASH_SIGNATURE_TABLE, key)
}

/// Canonical bugcheck form: `"133"`, `"0x00000133"`, `"NAME (133)"` → `"0x133"`.
pub fn normalize_bugcheck_code(raw: &str) -> Option<String> {
    let mut s = raw.trim();
    if let (Some(open), Some(close)) = (s.rfind('('), s.rfind(')')) {
        if open < close {
            s = s[open + 1..close].trim();
        }
    }
    let s = s.split('_').next().unwrap_or("");
    let s = s.trim_start_matches("0x").trim_start_matches("0X");
    if s.is_empty() || !s.chars().all(|c| c.is_ascii_hexdigit()) {
        return None;
    }
    u64::from_str_radix(s, 16).ok().map(|n| format!("{n:#x}"))
}

/// Lowercased module file name, preferring the image name when meaningful.
pub fn normalize_module(image_name: &str, module_name: &str) -> Option<String> {
    let clean = |v: &str| {
        let v = v.trim().to_ascii_lowercase();
        let bad = v.is_empty() || v.contains("unknown") || v == "none";
        (!bad).then_some(v)
    };
    clean(image_name).or_else(|| clean(module_name))
}

/// File stem without extension, lowercase: `rtwlane.sys` → `rtwlane`.
pub fn module_stem(module: &str) -> String {
    let m = module.trim().to_ascii_lowercase();
    match m.rsplit_once('.') {
        Some((stem, ext)) if matches!(ext, "sys" | "dll" | "exe" | "inf") => stem.to_string(),
        _ => m,
    }
}

fn field_after_colon(line: &str) -> String {
    line.split_once(':')
        .map(|(_, v)| v.trim().to_string())
        .unwrap_or_default()
}

fn offset_from_symbol(symbol: &str) -> Option<String> {
    let sym = symbol.trim();
    sym.split_once('+')
        .map(|(_, off)| format!("+{}", off.trim().trim_end_matches(')').trim()))
        .filter(|o| o.len() > 1)
}

fn looks_like_bugcheck_title(line: &str) -> bool {
    let t = line.trim();
    let Some(open) = t.find(" (") else { return false };
    t.ends_with(')')
        && open >= 5
        && t[..open]
            .chars()
            .all(|c| c.is_ascii_uppercase() || c.is_ascii_digit() || c == '_')
}

/// Extract crash fields from raw WinDbg `!analyze -v` text.
pub fn parse_windbg_analysis(text: &str) -> Option<ParsedCrash> {
    let mut p = ParsedCrash::default();
    let mut key_lines: Vec<&str> = Vec::new();
    for line in text.lines() {
        let t = line.trim();
        if t.starts_with("BUGCHECK_CODE:") {
            if let Some(code) = normalize_bugcheck_code(&field_after_colon(t)) {
                p.bugcheck_code = code;
            }
            key_lines.push(t);
        } else if t.starts_with("BUGCHECK_STR:") && p.bugcheck_code.is_empty() {
            if let Some(code) = normalize_bugcheck_code(&field_after_colon(t)) {
                p.bugcheck_code = code;
            }
            key_lines.push(t);
        } else if looks_like_bugcheck_title(t) && p.bugcheck_name.is_empty() {
            let open = t.find(" (").unwrap_or(t.len());
            p.bugcheck_name = t[..open].to_string();
            if p.bugcheck_code.is_empty() {
                if let Some(code) = normalize_bugcheck_code(t) {
                    p.bugcheck_code = code;
                }
            }
            key_lines.push(t);
        } else if t.starts_with("IMAGE_NAME:") {
            if let Some(m) = normalize_module(&field_after_colon(t), "") {
                p.module = m;
            }
            key_lines.push(t);
        } else if t.starts_with("MODULE_NAME:") {
            if p.module.is_empty() {
                if let Some(m) = normalize_module("", &field_after_colon(t)) {
                    p.module = m;
                }
            }
            key_lines.push(t);
        } else if t.starts_with("PROCESS_NAME:") {
            p.process_name = Some(field_after_colon(t)).filter(|v| !v.is_empty());
            key_lines.push(t);
        } else if t.starts_with("FAILURE_BUCKET_ID:") {
            p.failure_bucket = Some(field_after_colon(t)).filter(|v| !v.is_empty());
            key_lines.push(t);
        } else if t.starts_with("SYMBOL_NAME:") {
            if p.offset.is_none() {
                p.offset = offset_from_symbol(&field_after_colon(t));
            }
            key_lines.push(t);
        } else if t.starts_with("Probably caused by") {
            let v = field_after_colon(t);
            if !v.is_empty() {
                if p.module.is_empty() {
                    if let Some(m) =
                        normalize_module(v.split_whitespace().next().unwrap_or(""), "")
                    {
                        p.module = m;
                    }
                }
                if p.offset.is_none() {
                    p.offset = offset_from_symbol(&v);
                }
                p.caused_by = Some(v);
            }
            key_lines.push(t);
        }
    }
    p.raw_excerpt = key_lines.join("\n");
    (!p.bugcheck_code.is_empty() && !p.module.is_empty()).then_some(p)
}

/// Parse `===DUMP===`-chunked batch text into per-dump crashes.
pub fn parse_windbg_batch_text(text: &str) -> Vec<ParsedCrash> {
    text.split("===DUMP=== ")
        .filter_map(|chunk| {
            let (header, body) = chunk.split_once('\n')?;
            let mut parsed = parse_windbg_analysis(body)?;
            let mut parts = header.splitn(2, " | ");
            parsed.dump_name = parts.next().map(|s| s.trim().to_string());
            parsed.dump_time = parts.next().map(|s| s.trim().to_string());
            Some(parsed)
        })
        .collect()
}

fn json_str(v: &serde_json::Value, key: &str) -> String {
    v.get(key)
        .and_then(|x| x.as_str())
        .unwrap_or_default()
        .trim()
        .to_string()
}

/// `status` field of a dump-decode payload (`done` / `running` / …), if present.
pub fn payload_status(payload: &serde_json::Value) -> Option<String> {
    let data = payload.get("data").unwrap_or(payload);
    data.get("status")
        .and_then(|s| s.as_str())
        .map(|s| s.to_string())
}

/// Extract crashes from any `com.mastertech.dump-decode` result payload:
/// `read_batch` structured dumps, `read_analyze`/`read_analyze_livekernel`
/// label-prefixed fields, `read_raw` text heads, or bare WinDbg text.
pub fn parse_dump_decode_payload(payload: &serde_json::Value) -> Vec<ParsedCrash> {
    let data = payload.get("data").unwrap_or(payload);

    if let Some(dumps) = data.get("dumps").and_then(|d| d.as_array()) {
        return dumps
            .iter()
            .filter_map(|d| {
                let mut p = ParsedCrash {
                    bugcheck_code: normalize_bugcheck_code(&json_str(d, "bugcheck"))
                        .or_else(|| normalize_bugcheck_code(&json_str(d, "name")))
                        .unwrap_or_default(),
                    bugcheck_name: json_str(d, "name")
                        .split(" (")
                        .next()
                        .unwrap_or_default()
                        .to_string(),
                    module: normalize_module(&json_str(d, "image"), &json_str(d, "module"))
                        .unwrap_or_default(),
                    offset: offset_from_symbol(&json_str(d, "symbol")),
                    process_name: Some(json_str(d, "process")).filter(|v| !v.is_empty()),
                    failure_bucket: Some(json_str(d, "bucket")).filter(|v| !v.is_empty()),
                    caused_by: Some(json_str(d, "caused_by")).filter(|v| !v.is_empty()),
                    module_version: None,
                    dump_name: Some(json_str(d, "dump")).filter(|v| !v.is_empty()),
                    dump_time: Some(json_str(d, "time")).filter(|v| !v.is_empty()),
                    raw_excerpt: String::new(),
                    loaded_modules: Vec::new(),
                    triage: None,
                };
                if p.offset.is_none() {
                    p.offset = offset_from_symbol(&json_str(d, "caused_by"));
                }
                p.raw_excerpt = serde_json::to_string(d).unwrap_or_default();
                (!p.bugcheck_code.is_empty() && !p.module.is_empty()).then_some(p)
            })
            .collect();
    }

    if data.get("image_name").is_some()
        || data.get("bugcheck_str").is_some()
        || data.get("module_name").is_some()
    {
        let synthesized = format!(
            "{}\n{}\n{}\n{}\n{}",
            json_str(data, "bugcheck_str"),
            json_str(data, "failure_bucket"),
            json_str(data, "module_name"),
            json_str(data, "image_name"),
            json_str(data, "process_name"),
        );
        return parse_windbg_analysis(&synthesized).into_iter().collect();
    }

    let head = json_str(data, "head");
    if !head.is_empty() {
        return parse_windbg_batch_text(&head);
    }

    if let Some(text) = payload.as_str() {
        if text.contains("===DUMP===") {
            return parse_windbg_batch_text(text);
        }
        return parse_windbg_analysis(text).into_iter().collect();
    }

    Vec::new()
}

/// Extract crashes from a `dump-triage` result payload (native remote analysis,
/// local `minidump_analyze`, or the `com.mastertech.dump-triage` plugin).
/// Accepts `{dumps:[{dump_name, triage}...]}`, `{dump_name, triage}`, or a bare
/// `KernelDumpTriage` object. Each becomes a `ParsedCrash` carrying the full
/// triage blob + normalized loaded-module list for the sighting.
pub fn parse_kernel_triage_payload(payload: &serde_json::Value) -> Vec<ParsedCrash> {
    let data = payload.get("data").unwrap_or(payload);

    let items: Vec<(Option<String>, &serde_json::Value)> =
        if let Some(arr) = data.get("dumps").and_then(|d| d.as_array()) {
            arr.iter()
                .map(|d| {
                    let name = d
                        .get("dump_name")
                        .and_then(|v| v.as_str())
                        .map(str::to_string);
                    let triage = d.get("triage").unwrap_or(d);
                    (name, triage)
                })
                .collect()
        } else if let Some(triage) = data.get("triage") {
            let name = data.get("dump_name").and_then(|v| v.as_str()).map(str::to_string);
            vec![(name, triage)]
        } else if data.get("bugcheck_code").is_some() {
            let name = data.get("dump_name").and_then(|v| v.as_str()).map(str::to_string);
            vec![(name, data)]
        } else {
            return Vec::new();
        };

    items
        .into_iter()
        .filter_map(|(name, triage)| parsed_crash_from_triage(triage, name))
        .collect()
}

/// PE TimeDateStamp as a version marker: link date when plausible, hex otherwise.
fn pe_timestamp_version(ts: u32) -> String {
    use chrono::Datelike;
    match chrono::DateTime::<chrono::Utc>::from_timestamp(ts as i64, 0) {
        Some(d) if (1995..=2038).contains(&d.year()) => format!("built {}", d.format("%Y-%m-%d")),
        _ => format!("pe:{ts:#010x}"),
    }
}

/// Convert one `KernelDumpTriage` JSON object into a `ParsedCrash` via typed
/// deserialization. The original `Value` is persisted verbatim as `triage`.
fn parsed_crash_from_triage(
    t: &serde_json::Value,
    dump_name: Option<String>,
) -> Option<ParsedCrash> {
    let kd: dump_triage::KernelDumpTriage = serde_json::from_value(t.clone()).ok()?;
    let bugcheck_code = normalize_bugcheck_code(&kd.bugcheck_code)?;
    let module = kd
        .blamed_module
        .clone()
        .or_else(|| kd.rip_module.clone())
        .unwrap_or_else(|| "unknown".to_string());

    // caused_by/module_version only for a concrete third-party driver;
    // generic kernel-image blame (ntoskrnl/hal) carries no diagnostic value.
    let blamed_driver = kd
        .blamed_module
        .clone()
        .filter(|m| !dump_triage::is_kernel_image(m));
    let module_version = blamed_driver.as_deref().and_then(|m| {
        let ts = kd.drivers.iter().find(|d| d.name == m)?.timestamp?;
        (ts != 0).then(|| pe_timestamp_version(ts))
    });

    // Byte offset of RIP within its module, when both are known.
    let offset = match (kd.rip.as_deref(), &kd.rip_module) {
        (Some(rip), Some(rip_mod)) => {
            let rip = u64::from_str_radix(rip.trim_start_matches("0x"), 16).unwrap_or(0);
            kd.drivers
                .iter()
                .find(|d| d.name == *rip_mod)
                .map(|d| format!("+{:#x}", rip.saturating_sub(d.base)))
        }
        _ => None,
    };

    let mut loaded_modules: Vec<String> = kd.drivers.iter().map(|d| module_stem(&d.name)).collect();
    loaded_modules.sort();
    loaded_modules.dedup();

    let dump_time = kd
        .system_time_unix
        .and_then(|s| chrono::DateTime::<chrono::Utc>::from_timestamp(s, 0))
        .map(|d| d.format("%m/%d/%Y %H:%M UTC").to_string());

    let params = kd.bugcheck_parameters.join(", ");
    let raw_excerpt = format!(
        "{} ({}) params: {params} | rip: {} | blame: {} [dump-triage]",
        kd.bugcheck_name,
        kd.bugcheck_code,
        kd.rip.as_deref().unwrap_or("-"),
        kd.blamed_module.as_deref().unwrap_or("-"),
    );

    Some(ParsedCrash {
        bugcheck_code,
        bugcheck_name: kd.bugcheck_name.clone(),
        module,
        offset,
        process_name: None,
        failure_bucket: None,
        caused_by: blamed_driver,
        module_version,
        dump_name,
        dump_time,
        raw_excerpt,
        loaded_modules,
        triage: Some(t.clone()),
    })
}

const SIGNATURE_UPSERT: &str = "UPSERT $id MERGE { \
        bugcheck_code: $bugcheck_code, \
        bugcheck_name: IF $bugcheck_name != '' THEN $bugcheck_name ELSE (bugcheck_name ?? '') END, \
        module: $module, \
        offsets: array::distinct(array::concat(offsets ?? [], $offsets)), \
        module_versions: array::distinct(array::concat(module_versions ?? [], $module_versions)), \
        failure_buckets: array::distinct(array::concat(failure_buckets ?? [], $failure_buckets)), \
        machines: array::distinct(array::concat(machines ?? [], $machines)), \
        sighting_count: (sighting_count ?? 0) + 1, \
        first_seen: first_seen ?? time::now(), \
        last_seen: time::now() \
    } RETURN AFTER";

impl CrashSignature {
    /// Upsert the signature, append a sighting, and return prior fleet knowledge.
    /// Re-analyzing a dump already sighted on the same machine returns the
    /// existing state without double-counting.
    pub async fn ingest(parsed: &ParsedCrash, ctx: &SightingContext) -> anyhow::Result<CrashIngest> {
        let id = crash_signature_record_id(&parsed.bugcheck_code, &parsed.module);

        if let (Some(cs), Some(dn)) = (ctx.connection_string.as_deref(), parsed.dump_name.as_deref())
        {
            let existing: Vec<CrashSighting> = db()
                .query(
                    "SELECT * FROM crash_sighting WHERE signature == $sig \
                     AND connection_string == $cs AND dump_name == $dn LIMIT 1",
                )
                .bind(("sig", id.clone()))
                .bind(("cs", cs.to_string()))
                .bind(("dn", dn.to_string()))
                .await?
                .take(0)?;
            if let Some(prior) = existing.into_iter().next() {
                Self::backfill_sighting_links(&prior, ctx).await;
                if let Some(signature) = db().select::<Option<Self>>(id.clone()).await? {
                    let verdicts = Self::verdicts(&signature.id, 5).await?;
                    return Ok(CrashIngest {
                        previously_seen: true,
                        prior_sighting_count: signature.sighting_count,
                        prior_machine_count: signature.machines.len() as u32,
                        sighting_id: prior.id,
                        verdicts,
                        signature,
                    });
                }
            }
        }

        let machines: Vec<String> = ctx.connection_string.iter().cloned().collect();
        let offsets: Vec<String> = parsed.offset.iter().cloned().collect();
        let buckets: Vec<String> = parsed.failure_bucket.iter().cloned().collect();
        let versions: Vec<String> = parsed.module_version.iter().cloned().collect();

        let mut response = db()
            .query(SIGNATURE_UPSERT)
            .bind(("id", id.clone()))
            .bind(("bugcheck_code", parsed.bugcheck_code.clone()))
            .bind(("bugcheck_name", parsed.bugcheck_name.clone()))
            .bind(("module", parsed.module.clone()))
            .bind(("offsets", offsets))
            .bind(("module_versions", versions))
            .bind(("failure_buckets", buckets))
            .bind(("machines", machines))
            .await?;
        let rows: Vec<Self> = response.take(0)?;
        let signature = rows
            .into_iter()
            .next()
            .ok_or_else(|| anyhow::anyhow!("crash_signature UPSERT returned no row"))?;

        let prior_sighting_count = signature.sighting_count.saturating_sub(1);
        let new_machine = ctx
            .connection_string
            .as_ref()
            .map(|cs| signature.machines.iter().filter(|m| *m == cs).count() <= 1)
            .unwrap_or(false);
        let prior_machine_count =
            (signature.machines.len() as u32).saturating_sub(new_machine as u32);

        let sighting = CrashSighting {
            id: super::random_record_id(super::CRASH_SIGHTING_TABLE),
            signature: id.clone(),
            connection_string: ctx.connection_string.clone(),
            computer: ctx.computer.clone(),
            session_ref: ctx.session_ref.clone(),
            task_ref: ctx.task_ref.clone(),
            dump_name: parsed.dump_name.clone(),
            dump_kind: if ctx.dump_kind.is_empty() {
                "minidump".to_string()
            } else {
                ctx.dump_kind.clone()
            },
            dump_time: parsed.dump_time.clone(),
            offset: parsed.offset.clone(),
            module_version: parsed.module_version.clone(),
            failure_bucket: parsed.failure_bucket.clone(),
            process_name: parsed.process_name.clone(),
            caused_by: parsed.caused_by.clone(),
            raw_excerpt: parsed.raw_excerpt.chars().take(2000).collect(),
            loaded_modules: parsed.loaded_modules.clone(),
            triage: parsed.triage.clone(),
            created_at: chrono::Utc::now().into(),
        };
        let created: Option<CrashSighting> = db()
            .create(sighting.id.clone())
            .content(sighting.clone())
            .await?;
        let sighting_id = created.map(|s| s.id).unwrap_or(sighting.id);

        let verdicts = Self::verdicts(&signature.id, 5).await?;

        Ok(CrashIngest {
            previously_seen: prior_sighting_count > 0 || !verdicts.is_empty(),
            prior_sighting_count,
            prior_machine_count,
            sighting_id,
            verdicts,
            signature,
        })
    }

    /// Fill missing session/task/computer links on an existing sighting when a
    /// re-analysis carries them; never overwrites values already set.
    /// `loaded_modules ?? []` keeps UPDATE coercion happy on pre-schema rows.
    async fn backfill_sighting_links(prior: &CrashSighting, ctx: &SightingContext) {
        let wants_links = (prior.session_ref.is_none() && ctx.session_ref.is_some())
            || (prior.task_ref.is_none() && ctx.task_ref.is_some())
            || (prior.computer.is_none() && ctx.computer.is_some());
        if !wants_links {
            return;
        }
        let res = db()
            .query(
                "UPDATE $sighting SET \
                 session_ref = session_ref ?? $session_ref, \
                 task_ref = task_ref ?? $task_ref, \
                 computer = computer ?? $computer, \
                 loaded_modules = loaded_modules ?? []",
            )
            .bind(("sighting", prior.id.clone()))
            .bind(("session_ref", ctx.session_ref.clone()))
            .bind(("task_ref", ctx.task_ref.clone()))
            .bind(("computer", ctx.computer.clone()))
            .await;
        if let Err(e) = res {
            use super::RecordIdExt;
            log::warn!(
                "crash_sighting link backfill failed for {}: {e}",
                prior.id.key_string()
            );
        }
    }

    /// Fetch-or-create a signature without recording a sighting.
    pub async fn ensure(bugcheck_code: &str, module: &str) -> anyhow::Result<Self> {
        let code = normalize_bugcheck_code(bugcheck_code)
            .ok_or_else(|| anyhow::anyhow!("invalid bugcheck code '{bugcheck_code}'"))?;
        let module = module.trim().to_ascii_lowercase();
        if module.is_empty() {
            anyhow::bail!("module is required");
        }
        let id = crash_signature_record_id(&code, &module);
        if let Some(existing) = db().select(id.clone()).await? {
            return Ok(existing);
        }
        let mut response = db()
            .query(
                "UPSERT $id MERGE { bugcheck_code: $code, module: $module, \
                 sighting_count: sighting_count ?? 0, \
                 first_seen: first_seen ?? time::now(), last_seen: last_seen ?? time::now() } \
                 RETURN AFTER",
            )
            .bind(("id", id))
            .bind(("code", code))
            .bind(("module", module))
            .await?;
        let rows: Vec<Self> = response.take(0)?;
        rows.into_iter()
            .next()
            .ok_or_else(|| anyhow::anyhow!("crash_signature ensure returned no row"))
    }

    /// Signature lookup without recording a sighting.
    pub async fn find(bugcheck_code: &str, module: &str) -> anyhow::Result<Option<Self>> {
        let Some(code) = normalize_bugcheck_code(bugcheck_code) else {
            return Ok(None);
        };
        let id = crash_signature_record_id(&code, module);
        Ok(db().select(id).await?)
    }

    /// Newest verdicts for a signature.
    pub async fn verdicts(signature: &RecordId, limit: u32) -> anyhow::Result<Vec<CrashVerdict>> {
        let verdicts: Vec<CrashVerdict> = db()
            .query("SELECT * FROM crash_verdict WHERE signature == $sig ORDER BY created_at DESC LIMIT $limit")
            .bind(("sig", signature.clone()))
            .bind(("limit", limit as i64))
            .await?
            .take(0)?;
        Ok(verdicts)
    }

    /// Most recently seen signatures, for the intel browser.
    pub async fn recent(limit: u32) -> anyhow::Result<Vec<Self>> {
        let rows: Vec<Self> = db()
            .query("SELECT * FROM crash_signature ORDER BY last_seen DESC LIMIT $limit")
            .bind(("limit", limit as i64))
            .await?
            .take(0)?;
        Ok(rows)
    }

    /// Case-insensitive substring search over module and bucket names.
    pub async fn search(term: &str, limit: u32) -> anyhow::Result<Vec<Self>> {
        let rows: Vec<Self> = db()
            .query(
                "SELECT * FROM crash_signature \
                 WHERE string::contains(string::lowercase(module), string::lowercase($term)) \
                    OR string::contains(string::lowercase(bugcheck_code), string::lowercase($term)) \
                    OR string::contains(string::lowercase(bugcheck_name), string::lowercase($term)) \
                 ORDER BY last_seen DESC LIMIT $limit",
            )
            .bind(("term", term.to_string()))
            .bind(("limit", limit as i64))
            .await?
            .take(0)?;
        Ok(rows)
    }

    /// Sightings for a signature, newest first.
    pub async fn sightings(signature: &RecordId, limit: u32) -> anyhow::Result<Vec<CrashSighting>> {
        let rows: Vec<CrashSighting> = db()
            .query("SELECT * FROM crash_sighting WHERE signature == $sig ORDER BY created_at DESC LIMIT $limit")
            .bind(("sig", signature.clone()))
            .bind(("limit", limit as i64))
            .await?
            .take(0)?;
        Ok(rows)
    }
}

impl CrashVerdict {
    /// Record a verdict and point the signature's `latest_verdict` at it.
    pub async fn create(
        signature: &RecordId,
        verdict: &str,
        fix: &str,
        confidence: &str,
        author: &str,
        source: &str,
        task_ref: Option<RecordId>,
    ) -> anyhow::Result<RecordId> {
        let row = Self {
            id: super::random_record_id(super::CRASH_VERDICT_TABLE),
            signature: signature.clone(),
            verdict: verdict.to_string(),
            fix: fix.to_string(),
            confidence: confidence.to_string(),
            author: author.to_string(),
            source: source.to_string(),
            task_ref,
            created_at: chrono::Utc::now().into(),
        };
        let created: Option<Self> = db().create(row.id.clone()).content(row.clone()).await?;
        let id = created.map(|v| v.id).unwrap_or(row.id);

        db()
            .query("UPDATE $sig SET latest_verdict = $verdict")
            .bind(("sig", signature.clone()))
            .bind(("verdict", id.clone()))
            .await?;
        Ok(id)
    }
}

/// Claim orphan crash sightings and driver snapshots for a session and
/// propagate a late-arriving task link. Coalesce-only: existing links are
/// never overwritten. Orphan claims are bounded to the engagement span —
/// no earlier than 15 minutes before the session started and no later than
/// its end (now, while it is open) — so unlinked rows from other
/// engagements on the same connection are never mis-attributed.
/// `loaded_modules ?? []` keeps UPDATE coercion happy on pre-schema rows.
pub async fn reconcile_session_links(
    session: &super::diagnostic::DiagnosticSession,
) -> anyhow::Result<ReconcileReport> {
    let mut report = ReconcileReport::default();

    let claimed: Vec<CrashSighting> = db()
        .query(
            "UPDATE crash_sighting SET \
             session_ref = $sid, \
             task_ref = task_ref ?? $task, \
             computer = computer ?? $comp, \
             loaded_modules = loaded_modules ?? [] \
             WHERE connection_string == $cs AND session_ref == NONE \
             AND created_at >= ($started - 15m) \
             AND created_at <= ($ended ?? time::now())",
        )
        .bind(("sid", session.id.clone()))
        .bind(("task", session.task_ref.clone()))
        .bind(("comp", session.computer_id.clone()))
        .bind(("cs", session.connection_string.clone()))
        .bind(("started", session.started_at.clone()))
        .bind(("ended", session.ended_at.clone()))
        .await?
        .take(0)?;
    report.sightings_claimed = claimed.len();

    if session.task_ref.is_some() {
        let linked: Vec<CrashSighting> = db()
            .query(
                "UPDATE crash_sighting SET \
                 task_ref = $task, \
                 loaded_modules = loaded_modules ?? [] \
                 WHERE session_ref == $sid AND task_ref == NONE",
            )
            .bind(("task", session.task_ref.clone()))
            .bind(("sid", session.id.clone()))
            .await?
            .take(0)?;
        report.sightings_task_linked = linked.len();
    }

    let snapshots: Vec<super::driver_intel::DriverSnapshot> = db()
        .query(
            "UPDATE driver_snapshot SET \
             session_ref = $sid, \
             computer = computer ?? $comp \
             WHERE connection_string == $cs AND session_ref == NONE \
             AND taken_at >= ($started - 15m) \
             AND taken_at <= ($ended ?? time::now())",
        )
        .bind(("sid", session.id.clone()))
        .bind(("comp", session.computer_id.clone()))
        .bind(("cs", session.connection_string.clone()))
        .bind(("started", session.started_at.clone()))
        .bind(("ended", session.ended_at.clone()))
        .await?
        .take(0)?;
    report.snapshots_claimed = snapshots.len();

    report.sightings_enriched = enrich_session_dump_siblings(&session.id).await.unwrap_or(0);

    Ok(report)
}

/// Fill gaps between sightings of the SAME dump on one session. The fast
/// triage pass and the cdb deep pass produce separate rows (different blamed
/// module → different signature) for one dump: triage carries the forensic
/// blob + loaded modules + PE version, cdb carries the bucket + process +
/// probable cause. Each donates what the other lacks. Coalesce-only, so a
/// second run is a no-op. Signature links are never touched.
async fn enrich_session_dump_siblings(session_id: &RecordId) -> anyhow::Result<usize> {
    use super::RecordIdExt;
    let sightings = sightings_for_session(session_id).await?;
    let mut by_dump: std::collections::HashMap<String, Vec<CrashSighting>> =
        std::collections::HashMap::new();
    for s in sightings {
        let Some(dump) = s.dump_name.clone() else { continue };
        // Two analysis passes of ONE physical dump share both dump_name and
        // bugcheck code (signature key is "<code>_<module>"); two different
        // crashes reusing a fixed filename (C:\Windows\MEMORY.DMP) differ in
        // code and must NOT cross-donate. Key on both so only true siblings group.
        let sig_key = s.signature.key_string();
        let code = sig_key
            .split_once('_')
            .map(|(c, _)| c.to_string())
            .unwrap_or(sig_key);
        by_dump.entry(format!("{dump}::{code}")).or_default().push(s);
    }

    let mut enriched = 0usize;
    for (_dump, group) in by_dump {
        if group.len() < 2 {
            continue;
        }
        // Best available value for each donatable field across the group.
        let triage = group.iter().find_map(|s| s.triage.clone());
        let loaded = group
            .iter()
            .find(|s| !s.loaded_modules.is_empty())
            .map(|s| s.loaded_modules.clone());
        let module_version = group.iter().find_map(|s| s.module_version.clone());
        let failure_bucket = group.iter().find_map(|s| s.failure_bucket.clone());
        let process_name = group.iter().find_map(|s| s.process_name.clone());
        let caused_by = group.iter().find_map(|s| s.caused_by.clone());

        for s in &group {
            let wants = (s.triage.is_none() && triage.is_some())
                || (s.loaded_modules.is_empty() && loaded.is_some())
                || (s.module_version.is_none() && module_version.is_some())
                || (s.failure_bucket.is_none() && failure_bucket.is_some())
                || (s.process_name.is_none() && process_name.is_some())
                || (s.caused_by.is_none() && caused_by.is_some());
            if !wants {
                continue;
            }
            let res = db()
                .query(
                    "UPDATE $id SET \
                     triage = triage ?? $triage, \
                     loaded_modules = IF array::len(loaded_modules ?? []) > 0 \
                        THEN loaded_modules ELSE $loaded END, \
                     module_version = module_version ?? $mv, \
                     failure_bucket = failure_bucket ?? $fb, \
                     process_name = process_name ?? $pn, \
                     caused_by = caused_by ?? $cb",
                )
                .bind(("id", s.id.clone()))
                .bind(("triage", triage.clone()))
                .bind(("loaded", loaded.clone().unwrap_or_default()))
                .bind(("mv", module_version.clone()))
                .bind(("fb", failure_bucket.clone()))
                .bind(("pn", process_name.clone()))
                .bind(("cb", caused_by.clone()))
                .await;
            match res {
                Ok(_) => enriched += 1,
                Err(e) => {
                    log::warn!("sibling enrich failed for {}: {e}", s.id.key_string());
                }
            }
        }
    }
    Ok(enriched)
}

/// Fleet-wide unlinked-row counts: crash sightings and driver snapshots
/// with no `session_ref`. Surfaced by the link reaper as a health gauge.
pub async fn count_orphan_links() -> anyhow::Result<(usize, usize)> {
    let sightings: Vec<usize> = db()
        .query("SELECT VALUE count() FROM crash_sighting WHERE session_ref == NONE GROUP ALL")
        .await?
        .take(0)?;
    let snapshots: Vec<usize> = db()
        .query("SELECT VALUE count() FROM driver_snapshot WHERE session_ref == NONE GROUP ALL")
        .await?
        .take(0)?;
    Ok((
        sightings.into_iter().next().unwrap_or(0),
        snapshots.into_iter().next().unwrap_or(0),
    ))
}

/// Build a sighting context for a connected client from its open session:
/// connection_string + the session's computer/session/task links. Used by the
/// bench minidump view to link (and dedup) a manually-recorded sighting.
/// Returns a connection_string-only context when no open session exists.
pub async fn sighting_context_for_connection(
    connection_string: &str,
    dump_kind: &str,
) -> SightingContext {
    let session = super::diagnostic::DiagnosticSession::latest_open_for_connection(
        connection_string,
        None,
    )
    .await
    .ok()
    .flatten();
    let (computer, session_ref, task_ref) = match session {
        Some(s) => {
            let task = match s.task_ref.clone() {
                Some(t) => Some(t),
                None => s
                    .resolve_open_service_task()
                    .await
                    .ok()
                    .flatten()
                    .map(|(t, _)| t),
            };
            (Some(s.computer_id.clone()), Some(s.id.clone()), task)
        }
        None => (None, None, None),
    };
    SightingContext {
        connection_string: Some(connection_string.to_string()),
        computer,
        session_ref,
        task_ref,
        dump_kind: dump_kind.to_string(),
    }
}

/// Sightings recorded against a diagnostic session, newest first.
pub async fn sightings_for_session(
    session_id: &RecordId,
) -> anyhow::Result<Vec<CrashSighting>> {
    let rows: Vec<CrashSighting> = db()
        .query(
            "SELECT * FROM crash_sighting WHERE session_ref == $sid \
             ORDER BY created_at DESC LIMIT 100",
        )
        .bind(("sid", session_id.clone()))
        .await?
        .take(0)?;
    Ok(rows)
}

#[cfg(test)]
mod tests {
    use super::*;

    const ANALYZE_TEXT: &str = "\
Microsoft (R) Windows Debugger Version 10.0\n\
DPC_WATCHDOG_VIOLATION (133)\n\
The DPC watchdog detected a prolonged run time at an IRQL of DISPATCH_LEVEL.\n\
BUGCHECK_CODE:  133\n\
BUGCHECK_P1: 0\n\
PROCESS_NAME:  System\n\
MODULE_NAME: rtwlane\n\
IMAGE_NAME:  rtwlane.sys\n\
SYMBOL_NAME:  rtwlane+18e2b\n\
FAILURE_BUCKET_ID:  0x133_DPC_rtwlane!unknown_function\n\
Probably caused by : rtwlane.sys ( rtwlane+18e2b )\n";

    #[test]
    fn normalizes_bugcheck_codes() {
        assert_eq!(normalize_bugcheck_code("133").as_deref(), Some("0x133"));
        assert_eq!(normalize_bugcheck_code("0x00000133").as_deref(), Some("0x133"));
        assert_eq!(
            normalize_bugcheck_code("DPC_WATCHDOG_VIOLATION (133)").as_deref(),
            Some("0x133")
        );
        assert_eq!(normalize_bugcheck_code("0x1A").as_deref(), Some("0x1a"));
        assert_eq!(normalize_bugcheck_code("not hex"), None);
    }

    #[test]
    fn parses_analyze_text() {
        let p = parse_windbg_analysis(ANALYZE_TEXT).expect("should parse");
        assert_eq!(p.bugcheck_code, "0x133");
        assert_eq!(p.bugcheck_name, "DPC_WATCHDOG_VIOLATION");
        assert_eq!(p.module, "rtwlane.sys");
        assert_eq!(p.offset.as_deref(), Some("+18e2b"));
        assert_eq!(p.process_name.as_deref(), Some("System"));
        assert_eq!(
            p.failure_bucket.as_deref(),
            Some("0x133_DPC_rtwlane!unknown_function")
        );
    }

    #[test]
    fn parses_batch_chunks() {
        let text = format!(
            "CDB=C:\\cdb.exe\n===DUMP=== 071226-9375-01.dmp | 2026-07-12T09:33:00\n{ANALYZE_TEXT}\n===DUMP=== 071126-8562-01.dmp | 2026-07-11T18:02:11\n{ANALYZE_TEXT}"
        );
        let crashes = parse_windbg_batch_text(&text);
        assert_eq!(crashes.len(), 2);
        assert_eq!(crashes[0].dump_name.as_deref(), Some("071226-9375-01.dmp"));
        assert_eq!(crashes[1].dump_time.as_deref(), Some("2026-07-11T18:02:11"));
    }

    #[test]
    fn parses_read_batch_payload() {
        let payload = serde_json::json!({
            "tool": "read_batch",
            "data": {
                "status": "done",
                "analyzed": 1,
                "dumps": [{
                    "dump": "071226-9375-01.dmp",
                    "time": "2026-07-12T09:33:00",
                    "bugcheck": "133",
                    "name": "DPC_WATCHDOG_VIOLATION (133)",
                    "params": ["0", "501"],
                    "bucket": "0x133_DPC_rtwlane!unknown_function",
                    "module": "rtwlane",
                    "image": "rtwlane.sys",
                    "process": "System",
                    "symbol": "rtwlane+18e2b",
                    "caused_by": "rtwlane.sys ( rtwlane+18e2b )",
                    "stack": []
                }]
            }
        });
        assert_eq!(payload_status(&payload).as_deref(), Some("done"));
        let crashes = parse_dump_decode_payload(&payload);
        assert_eq!(crashes.len(), 1);
        assert_eq!(crashes[0].bugcheck_code, "0x133");
        assert_eq!(crashes[0].module, "rtwlane.sys");
        assert_eq!(crashes[0].offset.as_deref(), Some("+18e2b"));
    }

    #[test]
    fn parses_single_analyze_payload() {
        let payload = serde_json::json!({
            "tool": "read_analyze",
            "data": {
                "status": "done",
                "bugcheck_str": "BUGCHECK_STR:  0x1a_61941",
                "failure_bucket": "FAILURE_BUCKET_ID:  0x1a_61941_ntkrnlmp!unknown",
                "module_name": "MODULE_NAME: nt",
                "image_name": "IMAGE_NAME:  ntkrnlmp.exe",
                "process_name": "PROCESS_NAME:  chrome.exe"
            }
        });
        let crashes = parse_dump_decode_payload(&payload);
        assert_eq!(crashes.len(), 1);
        assert_eq!(crashes[0].bugcheck_code, "0x1a");
        assert_eq!(crashes[0].module, "ntkrnlmp.exe");
        assert_eq!(crashes[0].process_name.as_deref(), Some("chrome.exe"));
    }

    #[test]
    fn skips_unknown_modules() {
        assert_eq!(normalize_module("Unknown_Image", "Unknown_Module"), None);
        assert_eq!(
            normalize_module("", "rtwlane").as_deref(),
            Some("rtwlane")
        );
    }

    #[test]
    fn module_stems() {
        assert_eq!(module_stem("rtwlane.sys"), "rtwlane");
        assert_eq!(module_stem("RTWLANE.INF"), "rtwlane");
        assert_eq!(module_stem("nt"), "nt");
    }

    /// A wire-format `dump-triage` payload deserializes through the typed path and
    /// preserves the original triage blob verbatim on the sighting.
    #[test]
    fn typed_triage_extraction_preserves_contract() {
        let triage = serde_json::json!({
            "dump_type": 4,
            "dump_type_name": "triage_minidump",
            "bugcheck_code": "0x133",
            "bugcheck_name": "DPC_WATCHDOG_VIOLATION",
            "bugcheck_parameters": ["0x1", "0x1e00"],
            "rip": "0xfffff80320000100",
            "number_processors": 16,
            "registers": [["rip", "0xfffff80320000100"]],
            "system_time_unix": 1767225600i64,
            "drivers": [{
                "name": "rtwlane.sys",
                "path": "\\SystemRoot\\system32\\drivers\\rtwlane.sys",
                "base": 0xfffff80320000000u64,
                "size": 1048576u64,
                "timestamp": 1688256000u32
            }],
            "rip_module": "rtwlane.sys",
            "rip_in_kernel_image": false,
            "blamed_module": "rtwlane.sys"
        });
        let payload = serde_json::json!({ "dumps": [{ "dump_name": "070126-01.dmp", "triage": triage.clone() }] });
        let crashes = parse_kernel_triage_payload(&payload);
        assert_eq!(crashes.len(), 1);
        let c = &crashes[0];
        assert_eq!(c.bugcheck_code, "0x133");
        assert_eq!(c.bugcheck_name, "DPC_WATCHDOG_VIOLATION");
        assert_eq!(c.module, "rtwlane.sys");
        assert_eq!(c.offset.as_deref(), Some("+0x100"));
        assert_eq!(c.loaded_modules, vec!["rtwlane".to_string()]);
        assert_eq!(c.dump_name.as_deref(), Some("070126-01.dmp"));
        assert_eq!(c.caused_by.as_deref(), Some("rtwlane.sys"));
        assert_eq!(c.module_version.as_deref(), Some("built 2023-07-02"));
        assert_eq!(c.triage.as_ref(), Some(&triage));
    }

    /// Kernel-image blame keeps the signature module but sets no caused_by or
    /// module_version.
    #[test]
    fn kernel_image_blame_is_not_caused_by() {
        let triage = serde_json::json!({
            "bugcheck_code": "0x1a",
            "bugcheck_name": "MEMORY_MANAGEMENT",
            "rip": "0xfffff80310000200",
            "drivers": [{
                "name": "ntoskrnl.exe",
                "path": "\\SystemRoot\\system32\\ntoskrnl.exe",
                "base": 0xfffff80310000000u64,
                "size": 16777216u64,
                "timestamp": 1688256000u32
            }],
            "rip_module": "ntoskrnl.exe",
            "rip_in_kernel_image": true,
            "blamed_module": "ntoskrnl.exe"
        });
        let crashes = parse_kernel_triage_payload(&serde_json::json!({ "triage": triage }));
        assert_eq!(crashes.len(), 1);
        assert_eq!(crashes[0].module, "ntoskrnl.exe");
        assert_eq!(crashes[0].caused_by, None);
        assert_eq!(crashes[0].module_version, None);
    }

    #[test]
    fn pe_timestamp_versions() {
        assert_eq!(pe_timestamp_version(1688256000), "built 2023-07-02");
        assert_eq!(pe_timestamp_version(0xF0000000), "pe:0xf0000000");
    }
}
