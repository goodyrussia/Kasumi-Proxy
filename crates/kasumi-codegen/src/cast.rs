//! The old Tauri-layer export ran `dangerously_cast_bigints_to_number`: wire
//! numbers are JSON numbers and the UI is written against `number`, so Rust's
//! `usize`/`i64`/`u64`/… must not surface as TS `bigint`. Mirror that here so the
//! generated bindings keep the same numeric types they always had.

use specta::Types;
use specta::datatype::{DataType, Fields, Primitive};

/// Rewrite every BigInt-style primitive in the type graph to an equivalent
/// `number`-shaped one (JS `number` is exact well past the real values here).
pub fn cast_bigints_to_number(types: Types) -> Types {
    types.map(|mut ndt| {
        if let Some(ty) = ndt.ty.as_mut() {
            walk(ty);
        }
        ndt
    })
}

fn cast(p: Primitive) -> Primitive {
    match p {
        Primitive::usize | Primitive::u64 | Primitive::u128 => Primitive::u32,
        Primitive::isize | Primitive::i64 | Primitive::i128 => Primitive::i32,
        Primitive::f16 => Primitive::f32,
        other => other,
    }
}

fn walk(dt: &mut DataType) {
    match dt {
        DataType::Primitive(p) => *p = cast(p.clone()),
        DataType::List(l) => walk(&mut l.ty),
        DataType::Map(m) => {
            walk(m.key_ty_mut());
            walk(m.value_ty_mut());
        }
        DataType::Struct(s) => walk_fields(&mut s.fields),
        DataType::Enum(e) => {
            for (_, v) in &mut e.variants {
                walk_fields(&mut v.fields);
            }
        }
        DataType::Tuple(t) => {
            for el in &mut t.elements {
                walk(el);
            }
        }
        DataType::Nullable(inner) => walk(inner),
        DataType::Intersection(parts) => {
            for part in parts {
                walk(part);
            }
        }
        DataType::Generic(_) | DataType::Reference(_) => {}
    }
}

fn walk_fields(fields: &mut Fields) {
    match fields {
        Fields::Unit => {}
        Fields::Unnamed(u) => {
            for f in &mut u.fields {
                walk_field(f);
            }
        }
        Fields::Named(n) => {
            for (_, f) in &mut n.fields {
                walk_field(f);
            }
        }
    }
}

fn walk_field(f: &mut specta::datatype::Field) {
    if let Some(ty) = f.ty.as_mut() {
        walk(ty);
    }
}
