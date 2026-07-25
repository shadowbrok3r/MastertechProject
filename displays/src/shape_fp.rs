//! Runtime structural fingerprint of `Cmd` for the direct-TCP drift gate,
//! plus a `#[facet(sensitive)]`-aware formatter for command log sites.

use std::fmt;
use std::sync::LazyLock;

use facet::{Def, Facet, FieldIter, HasFields, Peek, StructKind, Type, UserType};

/// FNV-1a-64 of `Cmd`'s facet SHAPE, computed once on first access.
pub static CMD_SHAPE_FP: LazyLock<u64> =
    LazyLock::new(tcp_protocol::shape_fp::shape_fingerprint::<crate::Cmd>);

/// This build's crate version, sent alongside the fingerprint.
pub const BUILD_VERSION: &str = env!("CARGO_PKG_VERSION");

const MAX_DEPTH: usize = 12;

/// Wraps a `Facet` value so `Display` redacts `#[facet(sensitive)]` fields and
/// prints byte blobs as `<N bytes>`.
pub struct Redacted<'a, T: ?Sized>(&'a T);

/// Redacting `Display` wrapper for a command value in log output.
pub fn redacted<T: Facet<'static> + ?Sized>(value: &T) -> Redacted<'_, T> {
    Redacted(value)
}

impl<T: Facet<'static> + ?Sized> fmt::Display for Redacted<'_, T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        render(Peek::new(self.0), f, 0)
    }
}

fn render(peek: Peek<'_, '_>, f: &mut fmt::Formatter<'_>, depth: usize) -> fmt::Result {
    if depth >= MAX_DEPTH {
        return f.write_str("..");
    }
    let shape = peek.shape();
    match shape.def {
        Def::Option(_) => match peek.into_option().ok().and_then(|o| o.value()) {
            Some(inner) => render(inner, f, depth),
            None => f.write_str("None"),
        },
        Def::List(l) if l.t.is_type::<u8>() => {
            let n = peek.into_list().map(|list| list.len()).unwrap_or(0);
            write!(f, "<{n} bytes>")
        }
        Def::List(_) => {
            let Ok(list) = peek.into_list() else {
                return write!(f, "{peek:?}");
            };
            f.write_str("[")?;
            for (i, item) in list.iter().enumerate() {
                if i > 0 {
                    f.write_str(", ")?;
                }
                render(item, f, depth + 1)?;
            }
            f.write_str("]")
        }
        Def::Map(_) => {
            let n = peek.into_map().map(|m| m.len()).unwrap_or(0);
            write!(f, "{{{n} entries}}")
        }
        _ => match shape.ty {
            Type::User(UserType::Struct(_)) => {
                let Ok(s) = peek.into_struct() else {
                    return write!(f, "{peek:?}");
                };
                f.write_str(shape.type_identifier)?;
                render_fields(s.ty().kind, s.fields(), f, depth)
            }
            Type::User(UserType::Enum(_)) => {
                let Ok(e) = peek.into_enum() else {
                    return write!(f, "{peek:?}");
                };
                let Ok(variant) = e.active_variant() else {
                    return f.write_str(shape.type_identifier);
                };
                f.write_str(variant.rename.unwrap_or(variant.name))?;
                render_fields(variant.data.kind, e.fields(), f, depth)
            }
            _ => write!(f, "{peek:?}"),
        },
    }
}

fn render_fields(
    kind: StructKind,
    fields: FieldIter<'_, '_>,
    f: &mut fmt::Formatter<'_>,
    depth: usize,
) -> fmt::Result {
    let named = matches!(kind, StructKind::Struct);
    let mut first = true;
    for (field, value) in fields {
        f.write_str(if first {
            if named { " { " } else { "(" }
        } else {
            ", "
        })?;
        first = false;
        if named {
            write!(f, "{}: ", field.rename.unwrap_or(field.name))?;
        }
        if field.is_sensitive() {
            f.write_str("<redacted>")?;
        } else {
            render(value, f, depth + 1)?;
        }
    }
    if !first {
        f.write_str(if named { " }" } else { ")" })?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use facet::Facet;
    use tcp_protocol::shape_fp::shape_fingerprint;

    // bump deliberately when Cmd's wire shape changes; this is the drift review gate.
    #[test]
    fn cmd_shape_fp_pin() {
        assert_eq!(*super::CMD_SHAPE_FP, 0x8cb5_5a3e_9aba_98c3);
    }

    // bump deliberately when the dump-triage result contract changes.
    #[test]
    fn kernel_dump_triage_shape_fp_pin() {
        assert_eq!(shape_fingerprint::<dump_triage::KernelDumpTriage>(), 0xb781_c1ff_bb63_6a2d);
    }

    #[derive(Facet)]
    struct Baseline {
        a: u32,
        b: String,
    }
    #[derive(Facet)]
    struct Reordered {
        b: String,
        a: u32,
    }
    #[derive(Facet)]
    struct Retyped {
        a: u64,
        b: String,
    }
    #[derive(Facet)]
    struct MadeOptional {
        a: Option<u32>,
        b: String,
    }

    #[derive(Facet)]
    #[repr(u8)]
    enum BaseEnum {
        A,
        B,
    }
    #[derive(Facet)]
    #[repr(u8)]
    enum ExtendedEnum {
        A,
        B,
        C,
    }

    #[test]
    fn reordering_fields_changes_fingerprint() {
        assert_ne!(shape_fingerprint::<Baseline>(), shape_fingerprint::<Reordered>());
    }

    #[test]
    fn retyping_field_changes_fingerprint() {
        assert_ne!(shape_fingerprint::<Baseline>(), shape_fingerprint::<Retyped>());
    }

    #[test]
    fn making_field_optional_changes_fingerprint() {
        assert_ne!(shape_fingerprint::<Baseline>(), shape_fingerprint::<MadeOptional>());
    }

    #[test]
    fn adding_enum_variant_changes_fingerprint() {
        assert_ne!(shape_fingerprint::<BaseEnum>(), shape_fingerprint::<ExtendedEnum>());
    }

    #[derive(Facet)]
    struct Plain {
        a: u32,
        secret: Vec<u8>,
    }
    #[derive(Facet)]
    struct WithSensitive {
        a: u32,
        #[facet(sensitive)]
        secret: Vec<u8>,
    }

    #[test]
    fn sensitive_attribute_is_fingerprint_neutral() {
        assert_eq!(shape_fingerprint::<Plain>(), shape_fingerprint::<WithSensitive>());
    }

    // BuilderWire mirrors: keep in sync with plugin_builder::wire::BuilderWire;
    // BuilderWire::Hello has an extra worker_version field — do NOT assert equal
    // to Cmd::BuildWorkerHello.
    #[derive(Facet)]
    struct BuildWorkerHelloMirror {
        hostname: String,
        target_triples: Vec<String>,
        capabilities: Vec<String>,
    }
    #[derive(Facet)]
    struct CompilePluginRequestMirror {
        job_id: String,
        plugin_id: String,
        cargo_toml: String,
        lib_rs: String,
        target: String,
        profile: String,
    }
    #[derive(Facet)]
    struct CompilePluginProgressMirror {
        job_id: String,
        stage: String,
        message: String,
    }
    #[derive(Facet)]
    struct CompilePluginResultMirror {
        job_id: String,
        success: bool,
        wasm_bytes: Option<Vec<u8>>,
        stdout: String,
        stderr: String,
        duration_ms: u64,
    }

    #[test]
    fn build_worker_hello_pin() {
        assert_eq!(shape_fingerprint::<BuildWorkerHelloMirror>(), 0x6cb9_77f1_a467_9a8d);
    }

    #[test]
    fn compile_plugin_request_pin() {
        assert_eq!(shape_fingerprint::<CompilePluginRequestMirror>(), 0xb933_b6e2_65da_fcb4);
    }

    #[test]
    fn compile_plugin_progress_pin() {
        assert_eq!(shape_fingerprint::<CompilePluginProgressMirror>(), 0xf61b_ad4b_10e2_87b3);
    }

    #[test]
    fn compile_plugin_result_pin() {
        assert_eq!(shape_fingerprint::<CompilePluginResultMirror>(), 0x3cf2_9492_461b_21d7);
    }
}
