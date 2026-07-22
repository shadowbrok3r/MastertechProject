//! Reflection-driven walkers over a type's facet SHAPE: a serde_json field-diff
//! and a Peek-based read-only row generator.

use facet::{Def, Facet, Field, HasFields, Peek, Type, UserType};
use serde::Serialize;
use serde_json::Value;

/// Field name, serialized old value, serialized new value.
type Changed = (Field, Value, Value);

/// Walks `T::SHAPE` fields in declaration order and returns those whose
/// serialized value differs, skipping `ignore`, skip-serializing, and metadata
/// fields. One walk backs both `diff_to_json` and `has_changes`.
fn changed_fields<T: Facet<'static> + Serialize>(old: &T, new: &T, ignore: &[&str]) -> Vec<Changed> {
    let fields: &'static [Field] = match &T::SHAPE.ty {
        Type::User(UserType::Struct(st)) => st.fields,
        _ => return Vec::new(),
    };
    let (o, n) = match (serde_json::to_value(old), serde_json::to_value(new)) {
        (Ok(Value::Object(o)), Ok(Value::Object(n))) => (o, n),
        _ => return Vec::new(),
    };
    let null = Value::Null;
    let mut out = Vec::new();
    for field in fields {
        let key = field.name;
        if ignore.contains(&key)
            || field.should_skip_serializing_unconditional()
            || field.is_metadata()
        {
            continue;
        }
        let ov = o.get(key).unwrap_or(&null);
        let nv = n.get(key).unwrap_or(&null);
        if ov != nv {
            out.push((*field, ov.clone(), nv.clone()));
        }
    }
    out
}

/// Emits `{field: {"old": <string>, "new": <string>}}` for every serialized
/// field whose value differs. Field set and order come from `T::SHAPE`; values
/// from serde_json. `ignore` lists field names never diffed.
pub fn diff_to_json<T: Facet<'static> + Serialize>(old: &T, new: &T, ignore: &[&str]) -> Value {
    let mut map = serde_json::Map::new();
    for (field, ov, nv) in changed_fields(old, new, ignore) {
        map.insert(
            field.name.to_string(),
            serde_json::json!({ "old": render(&field, &ov), "new": render(&field, &nv) }),
        );
    }
    Value::Object(map)
}

/// True iff `diff_to_json(old, new, ignore)` would be non-empty.
pub fn has_changes<T: Facet<'static> + Serialize>(old: &T, new: &T, ignore: &[&str]) -> bool {
    !changed_fields(old, new, ignore).is_empty()
}

/// Formats a serialized field value as the string the diff contract requires.
fn render(field: &Field, v: &Value) -> String {
    if field.is_sensitive() {
        return "<redacted>".to_string();
    }
    render_value(v)
}

fn render_value(v: &Value) -> String {
    match v {
        Value::String(s) => s.clone(),
        Value::Bool(b) => b.to_string(),
        Value::Number(n) => n.to_string(),
        Value::Null => String::new(),
        Value::Object(m) if m.len() == 2 && m.contains_key("table") && m.contains_key("key") => {
            record_id_key_string(&m["key"])
        }
        Value::Object(_) | Value::Array(_) => serde_json::to_string(v).unwrap_or_default(),
    }
}

/// Reproduces `RecordIdExt::key_string()` from a serialized `RecordIdKey`.
fn record_id_key_string(key: &Value) -> String {
    if let Value::Object(m) = key {
        if let Some((_, inner)) = m.iter().next() {
            return match inner {
                Value::String(s) => s.clone(),
                Value::Number(n) => n.to_string(),
                other => serde_json::to_string(other).unwrap_or_default(),
            };
        }
    }
    match key {
        Value::String(s) => s.clone(),
        Value::Number(n) => n.to_string(),
        other => serde_json::to_string(other).unwrap_or_default(),
    }
}

/// A read-only (label, value) row with optional doc-comment hover text.
pub struct Row {
    pub label: String,
    pub value: String,
    pub hover: Option<String>,
}

/// One `Row` per scalar-ish top-level field of `value`. Compound fields (list,
/// map, nested struct, non-unit enum variant) are skipped. `Option<scalar>`
/// yields the inner value, or `""` when `None`.
pub fn rows<T: Facet<'static>>(value: &T) -> Vec<Row> {
    let peek = Peek::new(value);
    let Ok(s) = peek.into_struct() else {
        return Vec::new();
    };
    let mut out = Vec::new();
    for (field, fp) in s.fields() {
        if field.should_skip_serializing_unconditional() || field.is_metadata() {
            continue;
        }
        let Some(v) = row_value(&field, fp) else {
            continue;
        };
        let doc = normalize_doc(&field.doc.join(" "));
        out.push(Row {
            label: title_case(field.rename.unwrap_or(field.name)),
            value: v,
            hover: (!doc.is_empty()).then_some(doc),
        });
    }
    out
}

/// `Some(value)` for a scalar-ish field, `None` to skip a compound field.
fn row_value(field: &Field, peek: Peek) -> Option<String> {
    let v = scalar_string(peek)?;
    Some(if field.is_sensitive() {
        "<redacted>".to_string()
    } else {
        v
    })
}

fn scalar_string(peek: Peek) -> Option<String> {
    let shape = peek.shape();
    match shape.def {
        Def::Option(_) => match peek.into_option().ok().and_then(|o| o.value()) {
            Some(inner) => scalar_string(inner),
            None => Some(String::new()),
        },
        Def::List(l) if l.t.is_type::<u8>() => {
            let n = peek.into_list().map(|list| list.len()).unwrap_or(0);
            Some(format!("<{n} bytes>"))
        }
        Def::List(_) | Def::Slice(_) | Def::Array(_) | Def::Map(_) => None,
        Def::Scalar => Some(match peek.as_str() {
            Some(s) => s.to_string(),
            None => format!("{peek:?}"),
        }),
        _ => match shape.ty {
            Type::User(UserType::Enum(_)) => {
                let variant = peek.into_enum().ok()?.active_variant().ok()?;
                variant
                    .data
                    .fields
                    .is_empty()
                    .then(|| variant.rename.unwrap_or(variant.name).to_string())
            }
            _ => None,
        },
    }
}

/// Trims and single-space-joins doc text.
fn normalize_doc(raw: &str) -> String {
    raw.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// `_`-separated identifier to space-separated, first letter of each word upper.
fn title_case(name: &str) -> String {
    name.split('_')
        .map(|w| {
            let mut c = w.chars();
            match c.next() {
                Some(f) => f.to_uppercase().collect::<String>() + c.as_str(),
                None => String::new(),
            }
        })
        .collect::<Vec<_>>()
        .join(" ")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::schema::task::LiveTaskPayload;
    use crate::schema::RecordId;

    const IGNORE: &[&str] = &["id", "created_at"];

    fn value_is_string(v: &Value, key: &str, side: &str) -> bool {
        matches!(v.get(key).and_then(|c| c.get(side)), Some(Value::String(_)))
    }

    #[test]
    fn service_ticket_relink_detected() {
        let a = LiveTaskPayload {
            service_ticket: Some(RecordId::new("service_order", "a")),
            ..Default::default()
        };
        let b = LiveTaskPayload {
            service_ticket: Some(RecordId::new("service_order", "b")),
            ..a.clone()
        };
        assert!(has_changes(&a, &b, IGNORE));
        let diff = diff_to_json(&a, &b, IGNORE);
        assert!(diff.get("service_ticket").is_some());
        assert_eq!(diff["service_ticket"]["old"], Value::String("a".into()));
        assert_eq!(diff["service_ticket"]["new"], Value::String("b".into()));
    }

    #[test]
    fn identity_and_created_at_never_diffed() {
        let a = LiveTaskPayload::default();
        let b = LiveTaskPayload {
            id: RecordId::new("task", "other"),
            created_at: chrono::Utc::now().into(),
            ..a.clone()
        };
        assert!(!has_changes(&a, &b, IGNORE));
        assert_eq!(diff_to_json(&a, &b, IGNORE), Value::Object(Default::default()));
    }

    #[test]
    fn diff_contract_values_are_strings() {
        let a = LiveTaskPayload::default();
        let b = LiveTaskPayload {
            task_name: "renamed".into(),
            service_number: Some("SN-1".into()),
            status: crate::schema::task::Status::Complete,
            priority: crate::schema::task::Priority::Fire,
            completed: true,
            assignee: RecordId::new("user", "z"),
            ..a.clone()
        };
        let diff = diff_to_json(&a, &b, IGNORE);
        for key in ["task_name", "service_number", "status", "priority", "completed", "assignee"] {
            assert!(diff.get(key).is_some(), "missing {key}");
            assert!(value_is_string(&diff, key, "old"), "{key} old not string");
            assert!(value_is_string(&diff, key, "new"), "{key} new not string");
        }
        assert_eq!(diff["completed"]["new"], Value::String("true".into()));
        assert_eq!(diff["status"]["new"], Value::String("Complete".into()));
        assert_eq!(diff["priority"]["new"], Value::String("Fire".into()));
        assert_eq!(diff["assignee"]["new"], Value::String("z".into()));
    }

    #[test]
    fn due_date_diff_renders_readable_string() {
        use chrono::TimeZone;
        let a = LiveTaskPayload {
            due_date: chrono::Utc.with_ymd_and_hms(2020, 1, 1, 0, 0, 0).unwrap().into(),
            ..Default::default()
        };
        let b = LiveTaskPayload {
            due_date: chrono::Utc.with_ymd_and_hms(2021, 6, 15, 12, 30, 0).unwrap().into(),
            ..a.clone()
        };
        assert!(has_changes(&a, &b, IGNORE));
        let diff = diff_to_json(&a, &b, IGNORE);
        assert!(diff.get("due_date").is_some(), "due_date not emitted");
        assert!(value_is_string(&diff, "due_date", "old"), "due_date old not string");
        assert!(value_is_string(&diff, "due_date", "new"), "due_date new not string");
        let old = diff["due_date"]["old"].as_str().unwrap();
        let new = diff["due_date"]["new"].as_str().unwrap();
        assert_ne!(old, new);
        assert!(old.contains("2020"), "old not RFC3339: {old}");
        assert!(new.contains("2021"), "new not RFC3339: {new}");
    }

    #[test]
    fn no_changes_on_equal() {
        let a = LiveTaskPayload::default();
        assert!(!has_changes(&a, &a.clone(), IGNORE));
    }

    #[derive(Facet)]
    struct RowFixture {
        /// Peak temperature.
        cpu_max: f64,
        #[facet(rename = "System Name")]
        hostname: String,
        note: Option<String>,
        #[facet(sensitive)]
        api_key: String,
        tags: Vec<String>,
    }

    #[test]
    fn rows_scalar_labels_hover_and_skips() {
        let fx = RowFixture {
            cpu_max: 70.0,
            hostname: "PC-1".into(),
            note: None,
            api_key: "secret".into(),
            tags: vec!["a".into()],
        };
        let rows = rows(&fx);
        let by_label = |l: &str| rows.iter().find(|r| r.label == l);

        let cpu = by_label("Cpu Max").expect("cpu_max row");
        assert_eq!(cpu.value, "70.0");
        assert_eq!(cpu.hover.as_deref(), Some("Peak temperature."));

        assert_eq!(by_label("System Name").unwrap().value, "PC-1");
        assert_eq!(by_label("Note").unwrap().value, "");
        assert_eq!(by_label("Api Key").unwrap().value, "<redacted>");
        assert!(by_label("Tags").is_none());
    }
}
