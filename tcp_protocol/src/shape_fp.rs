//! Structural fingerprint of a single type's facet SHAPE.
//!
//! FNV-1a-64 over a canonical token string keyed on field names/renames,
//! types, declaration order, and optionality — doc text and `#[repr]` are
//! excluded. Maps tokenize as an opaque `M`, foreign/opaque leaves as `O`,
//! scalars as their `type_identifier`. The `token`/`token_struct`/
//! `token_enum`/`fnv1a_64` walk is shared verbatim with
//! `mtech-plugin-sdk::schema` so fingerprints are comparable ecosystem-wide.

use facet::{Def, EnumType, Field, Shape, StructType, Type, UserType};

/// FNV-1a-64 of the canonical token string for `T`'s SHAPE.
pub fn shape_fingerprint<T: facet::Facet<'static>>() -> u64 {
    let mut s = String::new();
    token(&mut s, T::SHAPE, 0);
    fnv1a_64(s.as_bytes())
}

const DEPTH_CAP: u8 = 8;

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

fn field_key(f: &Field) -> &'static str {
    f.rename.unwrap_or(f.name)
}

fn is_optional(f: &Field) -> bool {
    matches!(f.shape().def, Def::Option(_)) || f.has_default()
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

    #[test]
    fn fnv1a_known_vectors() {
        assert_eq!(fnv1a_64(b""), 0xcbf2_9ce4_8422_2325);
        assert_eq!(fnv1a_64(b"a"), 0xaf63_dc4c_8601_ec8c);
        assert_eq!(fnv1a_64(b"foobar"), 0x8594_4171_f739_67e8);
    }
}
