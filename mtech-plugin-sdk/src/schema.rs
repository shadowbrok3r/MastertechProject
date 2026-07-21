//! Static SHAPE walk: JSON-schema generation and structural fingerprint.

use crate::dispatch::normalize_doc;
use facet::{Def, EnumType, Field, Shape, StructType, Type, UserType};

/// One advertised tool: name, raw doc lines, and optional argument shape.
pub struct ToolDef {
    pub name: &'static str,
    pub description: &'static str,
    pub args: Option<&'static Shape>,
}

/// Ordered set of tool definitions collected by the macro.
pub struct ToolSet(Vec<ToolDef>);

impl ToolSet {
    pub fn new() -> Self {
        Self(Vec::new())
    }

    pub fn push(&mut self, name: &'static str, raw_doc: &'static str, args: Option<&'static Shape>) {
        self.0.push(ToolDef { name, description: raw_doc, args });
    }

    /// Emits `[{"name","description","parameters_schema"}, ...]` in declaration order.
    pub fn to_tools_json(&self) -> String {
        let mut out = String::from("[");
        for (i, td) in self.0.iter().enumerate() {
            if i > 0 {
                out.push(',');
            }
            out.push_str("{\"name\":\"");
            json_escape(&mut out, td.name);
            out.push_str("\",\"description\":\"");
            json_escape(&mut out, &normalize_doc(td.description));
            out.push_str("\",\"parameters_schema\":");
            out.push_str(&tool_schema_json(td.args));
            out.push('}');
        }
        out.push(']');
        out
    }

    /// FNV-1a-64 over a canonical structural string (names/types/optionality only).
    pub fn fingerprint(&self) -> u64 {
        let mut s = String::from("abi:");
        push_u32(&mut s, crate::ABI_VERSION);
        s.push(';');
        for td in &self.0 {
            s.push_str("tool:");
            s.push_str(td.name);
            s.push(':');
            match td.args {
                None => s.push_str("()"),
                Some(shape) => token(&mut s, shape, 0),
            }
            s.push(';');
        }
        fnv1a_64(s.as_bytes())
    }
}

impl Default for ToolSet {
    fn default() -> Self {
        Self::new()
    }
}

/// JSON schema object for a tool's arguments; `None` = no-arg tool.
pub fn tool_schema_json(args: Option<&'static Shape>) -> String {
    match args {
        None => String::from("{\"type\":\"object\",\"properties\":{}}"),
        Some(shape) => {
            let mut out = String::new();
            write_shape(&mut out, shape, 0);
            out
        }
    }
}

const DEPTH_CAP: u8 = 8;

fn write_shape(out: &mut String, shape: &'static Shape, depth: u8) {
    if depth >= DEPTH_CAP {
        out.push_str("{\"type\":\"object\"}");
        return;
    }
    match shape.def {
        Def::Scalar => {
            out.push_str("{\"type\":\"");
            out.push_str(scalar_type(shape.type_identifier));
            out.push_str("\"}");
        }
        Def::Option(o) => write_shape(out, o.t, depth),
        Def::List(l) => write_array(out, l.t, depth),
        Def::Slice(l) => write_array(out, l.t, depth),
        Def::Array(a) => write_array(out, a.t, depth),
        Def::Map(_) => out.push_str("{\"type\":\"object\"}"),
        _ => match shape.ty {
            Type::User(UserType::Struct(ref s)) => write_struct(out, s, depth),
            Type::User(UserType::Enum(ref e)) => write_enum(out, e),
            _ => out.push_str("{\"type\":\"object\"}"),
        },
    }
}

fn write_array(out: &mut String, elem: &'static Shape, depth: u8) {
    out.push_str("{\"type\":\"array\",\"items\":");
    write_shape(out, elem, depth + 1);
    out.push('}');
}

fn write_struct(out: &mut String, s: &StructType, depth: u8) {
    out.push_str("{\"type\":\"object\",\"properties\":{");
    for (i, f) in s.fields.iter().enumerate() {
        if i > 0 {
            out.push(',');
        }
        out.push('"');
        json_escape(out, field_key(f));
        out.push_str("\":");
        write_field_node(out, f, depth + 1);
    }
    out.push('}');
    let req: Vec<&str> = s.fields.iter().filter(|f| !is_optional(f)).map(field_key).collect();
    if !req.is_empty() {
        out.push_str(",\"required\":[");
        for (i, k) in req.iter().enumerate() {
            if i > 0 {
                out.push(',');
            }
            out.push('"');
            json_escape(out, k);
            out.push('"');
        }
        out.push(']');
    }
    out.push('}');
}

fn write_field_node(out: &mut String, f: &Field, depth: u8) {
    let mut node = String::new();
    write_shape(&mut node, f.shape(), depth);
    let desc = normalize_doc(&f.doc.join(" "));
    if desc.is_empty() || !node.starts_with('{') || !node.ends_with('}') || node.len() < 2 {
        out.push_str(&node);
        return;
    }
    let inner = &node[1..node.len() - 1];
    out.push('{');
    if !inner.is_empty() {
        out.push_str(inner);
        out.push(',');
    }
    out.push_str("\"description\":\"");
    json_escape(out, &desc);
    out.push_str("\"}");
}

fn write_enum(out: &mut String, e: &EnumType) {
    if e.variants.iter().all(|v| v.data.fields.is_empty()) {
        out.push_str("{\"type\":\"string\",\"enum\":[");
        for (i, v) in e.variants.iter().enumerate() {
            if i > 0 {
                out.push(',');
            }
            out.push('"');
            json_escape(out, v.rename.unwrap_or(v.name));
            out.push('"');
        }
        out.push_str("]}");
    } else {
        out.push_str("{\"type\":\"object\",\"description\":\"one of variants: ");
        for (i, v) in e.variants.iter().enumerate() {
            if i > 0 {
                out.push_str(", ");
            }
            json_escape(out, v.rename.unwrap_or(v.name));
        }
        out.push_str("\"}");
    }
}

fn scalar_type(id: &str) -> &'static str {
    match id {
        "String" | "str" | "&str" | "char" => "string",
        "bool" => "boolean",
        "f32" | "f64" => "number",
        "u8" | "u16" | "u32" | "u64" | "u128" | "usize" | "i8" | "i16" | "i32" | "i64" | "i128"
        | "isize" => "integer",
        _ => "string",
    }
}

fn field_key(f: &Field) -> &'static str {
    f.rename.unwrap_or(f.name)
}

fn is_optional(f: &Field) -> bool {
    matches!(f.shape().def, Def::Option(_)) || f.has_default()
}

pub(crate) fn json_escape(out: &mut String, s: &str) {
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if (c as u32) < 0x20 => {
                out.push_str("\\u");
                for shift in [12u32, 8, 4, 0] {
                    let nib = (c as u32 >> shift) & 0xf;
                    out.push(char::from_digit(nib, 16).unwrap());
                }
            }
            c => out.push(c),
        }
    }
}

fn push_u32(out: &mut String, mut n: u32) {
    if n == 0 {
        out.push('0');
        return;
    }
    let mut buf = [0u8; 10];
    let mut i = buf.len();
    while n > 0 {
        i -= 1;
        buf[i] = b'0' + (n % 10) as u8;
        n /= 10;
    }
    out.push_str(std::str::from_utf8(&buf[i..]).unwrap());
}

const FNV_OFFSET: u64 = 0xcbf2_9ce4_8422_2325;
const FNV_PRIME: u64 = 0x0000_0100_0000_01b3;

fn fnv1a_64(bytes: &[u8]) -> u64 {
    let mut h = FNV_OFFSET;
    for &b in bytes {
        h ^= b as u64;
        h = h.wrapping_mul(FNV_PRIME);
    }
    h
}

fn token(out: &mut String, shape: &'static Shape, depth: u8) {
    if depth >= DEPTH_CAP {
        out.push('!');
        return;
    }
    match shape.def {
        Def::Scalar => out.push_str(shape.type_identifier),
        Def::Option(o) => {
            out.push('?');
            token(out, o.t, depth);
        }
        Def::List(l) => {
            out.push('[');
            token(out, l.t, depth + 1);
            out.push(']');
        }
        Def::Slice(l) => {
            out.push('[');
            token(out, l.t, depth + 1);
            out.push(']');
        }
        Def::Array(a) => {
            out.push('[');
            token(out, a.t, depth + 1);
            out.push(']');
        }
        Def::Map(_) => out.push('M'),
        _ => match shape.ty {
            Type::User(UserType::Struct(ref s)) => token_struct(out, s, depth),
            Type::User(UserType::Enum(ref e)) => token_enum(out, e, depth),
            _ => out.push('O'),
        },
    }
}

fn token_struct(out: &mut String, s: &StructType, depth: u8) {
    out.push_str("S{");
    for f in s.fields {
        out.push_str(field_key(f));
        out.push(':');
        if is_optional(f) {
            out.push('?');
        }
        token(out, f.shape(), depth + 1);
        out.push(',');
    }
    out.push('}');
}

fn token_enum(out: &mut String, e: &EnumType, depth: u8) {
    if e.variants.iter().all(|v| v.data.fields.is_empty()) {
        out.push_str("E[");
        for (i, v) in e.variants.iter().enumerate() {
            if i > 0 {
                out.push(',');
            }
            out.push_str(v.rename.unwrap_or(v.name));
        }
        out.push(']');
    } else {
        out.push_str("E{");
        for v in e.variants {
            out.push_str(v.rename.unwrap_or(v.name));
            out.push(':');
            token_struct(out, &v.data, depth + 1);
            out.push(',');
        }
        out.push('}');
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use facet::Facet;
    use serde::Deserialize;

    #[derive(Facet, Deserialize)]
    struct Simple {
        /// The published INF name.
        published_name: String,
        /// Optional timeout.
        timeout_secs: Option<u64>,
        count: u32,
        enabled: bool,
        ratio: f64,
        tags: Vec<String>,
    }

    #[derive(Facet, Deserialize)]
    struct Nested {
        inner: Simple,
        items: Vec<Simple>,
    }

    #[derive(Facet, Deserialize)]
    #[repr(u8)]
    #[allow(dead_code)]
    enum Mode {
        Fast,
        Slow,
        #[facet(rename = "very_slow")]
        VerySlow,
    }

    #[derive(Facet, Deserialize)]
    struct WithEnum {
        mode: Mode,
    }

    #[derive(Facet, Deserialize)]
    struct WithDefault {
        #[facet(default)]
        maybe: u32,
    }

    fn shape_json<T: Facet<'static>>() -> serde_json::Value {
        serde_json::from_str(&tool_schema_json(Some(T::SHAPE))).unwrap()
    }

    #[test]
    fn scalar_table() {
        let v = shape_json::<Simple>();
        let props = &v["properties"];
        assert_eq!(props["published_name"]["type"], "string");
        assert_eq!(props["count"]["type"], "integer");
        assert_eq!(props["enabled"]["type"], "boolean");
        assert_eq!(props["ratio"]["type"], "number");
        assert_eq!(props["timeout_secs"]["type"], "integer");
    }

    #[test]
    fn option_present_in_properties_absent_from_required() {
        let v = shape_json::<Simple>();
        assert!(v["properties"].get("timeout_secs").is_some());
        let req: Vec<&str> = v["required"].as_array().unwrap().iter().map(|x| x.as_str().unwrap()).collect();
        assert!(!req.contains(&"timeout_secs"));
        assert!(req.contains(&"published_name"));
        assert!(req.contains(&"count"));
    }

    #[test]
    fn default_field_absent_from_required() {
        let v = shape_json::<WithDefault>();
        assert!(v["properties"].get("maybe").is_some());
        assert!(v.get("required").is_none());
    }

    #[test]
    fn vec_maps_to_array_items() {
        let v = shape_json::<Simple>();
        assert_eq!(v["properties"]["tags"]["type"], "array");
        assert_eq!(v["properties"]["tags"]["items"]["type"], "string");
    }

    #[test]
    fn nested_struct_recurses() {
        let v = shape_json::<Nested>();
        assert_eq!(v["properties"]["inner"]["type"], "object");
        assert_eq!(v["properties"]["inner"]["properties"]["count"]["type"], "integer");
        assert_eq!(v["properties"]["items"]["type"], "array");
        assert_eq!(v["properties"]["items"]["items"]["type"], "object");
    }

    #[test]
    fn doc_lines_become_description() {
        let v = shape_json::<Simple>();
        assert_eq!(v["properties"]["published_name"]["description"], "The published INF name.");
    }

    #[test]
    fn unit_enum_maps_to_string_enum_with_rename() {
        let v = shape_json::<WithEnum>();
        let mode = &v["properties"]["mode"];
        assert_eq!(mode["type"], "string");
        let variants: Vec<&str> = mode["enum"].as_array().unwrap().iter().map(|x| x.as_str().unwrap()).collect();
        assert_eq!(variants, vec!["Fast", "Slow", "very_slow"]);
    }

    #[test]
    fn no_arg_schema() {
        assert_eq!(tool_schema_json(None), "{\"type\":\"object\",\"properties\":{}}");
    }

    #[test]
    fn json_escape_handles_control_chars() {
        let mut s = String::new();
        json_escape(&mut s, "a\"b\\c\nd\te\u{1}f");
        assert_eq!(s, "a\\\"b\\\\c\\nd\\te\\u0001f");
    }

    #[test]
    fn tools_json_deserializes_and_preserves_order() {
        #[derive(serde::Deserialize)]
        struct Descriptor {
            name: String,
            description: String,
            parameters_schema: serde_json::Value,
        }
        let mut ts = ToolSet::new();
        ts.push("beta", " Second   tool.", None);
        ts.push("alpha", " First tool.", Some(Simple::SHAPE));
        let json = ts.to_tools_json();
        let parsed: Vec<Descriptor> = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed.len(), 2);
        assert_eq!(parsed[0].name, "beta");
        assert_eq!(parsed[0].description, "Second tool.");
        assert_eq!(parsed[0].parameters_schema["properties"], serde_json::json!({}));
        assert_eq!(parsed[1].name, "alpha");
        assert_eq!(parsed[1].parameters_schema["properties"]["published_name"]["type"], "string");
    }

    #[test]
    fn fingerprint_stable() {
        let mut a = ToolSet::new();
        a.push("t", " doc", Some(Simple::SHAPE));
        let mut b = ToolSet::new();
        b.push("t", " doc", Some(Simple::SHAPE));
        assert_eq!(a.fingerprint(), b.fingerprint());
    }

    #[test]
    fn fingerprint_ignores_doc_text() {
        let mut a = ToolSet::new();
        a.push("t", " original doc", Some(Simple::SHAPE));
        let mut b = ToolSet::new();
        b.push("t", " completely reworded documentation", Some(Simple::SHAPE));
        assert_eq!(a.fingerprint(), b.fingerprint());
    }

    #[derive(Facet, Deserialize)]
    struct Retyped {
        published_name: String,
        timeout_secs: Option<u64>,
        count: u64,
        enabled: bool,
        ratio: f64,
        tags: Vec<String>,
    }

    #[derive(Facet, Deserialize)]
    struct Reordered {
        count: u32,
        published_name: String,
        timeout_secs: Option<u64>,
        enabled: bool,
        ratio: f64,
        tags: Vec<String>,
    }

    #[derive(Facet, Deserialize)]
    struct MadeOptional {
        published_name: Option<String>,
        timeout_secs: Option<u64>,
        count: u32,
        enabled: bool,
        ratio: f64,
        tags: Vec<String>,
    }

    #[test]
    fn fingerprint_changes_on_structural_edits() {
        let base = {
            let mut t = ToolSet::new();
            t.push("t", "d", Some(Simple::SHAPE));
            t.fingerprint()
        };
        let retyped = {
            let mut t = ToolSet::new();
            t.push("t", "d", Some(Retyped::SHAPE));
            t.fingerprint()
        };
        let reordered = {
            let mut t = ToolSet::new();
            t.push("t", "d", Some(Reordered::SHAPE));
            t.fingerprint()
        };
        let optional = {
            let mut t = ToolSet::new();
            t.push("t", "d", Some(MadeOptional::SHAPE));
            t.fingerprint()
        };
        let added = {
            let mut t = ToolSet::new();
            t.push("t", "d", Some(Simple::SHAPE));
            t.push("t2", "d", None);
            t.fingerprint()
        };
        assert_ne!(base, retyped);
        assert_ne!(base, reordered);
        assert_ne!(base, optional);
        assert_ne!(base, added);
    }

    #[derive(Facet, Deserialize)]
    #[repr(u8)]
    #[allow(dead_code)]
    enum ModeRenamed {
        Fast,
        Slow,
        Turbo,
    }

    #[derive(Facet, Deserialize)]
    struct WithModeRenamed {
        mode: ModeRenamed,
    }

    #[test]
    fn fingerprint_changes_on_enum_variant_change() {
        let a = {
            let mut t = ToolSet::new();
            t.push("t", "d", Some(WithEnum::SHAPE));
            t.fingerprint()
        };
        let b = {
            let mut t = ToolSet::new();
            t.push("t", "d", Some(WithModeRenamed::SHAPE));
            t.fingerprint()
        };
        assert_ne!(a, b);
    }

    #[test]
    fn fnv1a_known_vectors() {
        assert_eq!(fnv1a_64(b""), 0xcbf2_9ce4_8422_2325);
        assert_eq!(fnv1a_64(b"a"), 0xaf63_dc4c_8601_ec8c);
        assert_eq!(fnv1a_64(b"foobar"), 0x8594_4171_f739_67e8);
    }
}
