//! Swift phon descriptor generation.
//!
//! Emits, for a facet `Shape`, a Swift expression that builds a
//! `PhonIR.Descriptor` — the typed-path codec's input. The tricky witness logic
//! lives in the phon package's generic factories (`OptionWitness.of`,
//! `BytesWitness.string`, `SeqWitness.of`, `MapWitness.stringKeyed`); enum
//! `tag`/`project`/`destroy`/`inject` are emitted per-type here (they switch over
//! concrete cases).
//!
//! First cut covers what the Message envelope + the core Testbed need: scalars,
//! String/bytes, struct/tuple-struct records, Option, list (bulk scalar /
//! per-element struct), string-keyed map, enum (unit + newtype variants), and
//! Result. Unsupported shapes panic at codegen time (loud, not silent).

use facet_core::{ScalarType, Shape};
use vox_types::{
    EnumInfo, ShapeKind, StructInfo, VariantKind, classify_shape, classify_variant,
    extract_schemas, is_bytes,
};

use super::types::{swift_field_name, swift_type_base};
use crate::render::hex_u64;

/// The phon content-id (`SchemaId`) of a shape's root.
fn phon_schema_id(shape: &'static Shape) -> u64 {
    use vox_types::TypeRef;
    match extract_schemas(shape).expect("phon schema extraction").root {
        TypeRef::Concrete { type_id, .. } => type_id.0,
        TypeRef::Var { .. } => panic!("phon descriptor root cannot be a type variable"),
    }
}

/// `MemoryLayout<T>` size/align expression for a shape's Swift type.
fn layout_expr(shape: &'static Shape) -> String {
    let ty = swift_type_base(shape);
    format!("Layout(size: MemoryLayout<{ty}>.size, align: MemoryLayout<{ty}>.alignment)")
}

fn schema_ref(shape: &'static Shape) -> String {
    format!(".concrete(SchemaId({}))", hex_u64(phon_schema_id(shape)))
}

/// Whether a shape is a fixed-width scalar (copies as raw bytes) — bool, the
/// integer and float widths, char/unit. String/Str are NOT fixed (bulk bytes).
fn is_fixed_scalar(shape: &'static Shape) -> bool {
    matches!(
        classify_shape(shape),
        ShapeKind::Scalar(
            ScalarType::Unit
                | ScalarType::Bool
                | ScalarType::U8
                | ScalarType::U16
                | ScalarType::U32
                | ScalarType::U64
                | ScalarType::USize
                | ScalarType::I8
                | ScalarType::I16
                | ScalarType::I32
                | ScalarType::I64
                | ScalarType::ISize
                | ScalarType::F32
                | ScalarType::F64
        )
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use facet::Facet;

    #[test]
    fn scalar_and_string() {
        assert_eq!(access_expr(<u32 as Facet>::SHAPE), ".scalar");
        assert!(access_expr(<String as Facet>::SHAPE).contains("witness: .string"));
    }

    #[test]
    fn envelope_descriptor_emits() {
        // Walk the real Message envelope — panics here name the unsupported shape.
        let expr = descriptor_expr(<vox_types::Message<'static> as Facet>::SHAPE);
        assert!(expr.contains("Descriptor("), "got: {expr}");
        assert!(
            expr.contains(".enumeration"),
            "envelope should be an enum at the root payload"
        );
    }
}

/// The full `Descriptor(...)` expression for a shape.
pub fn descriptor_expr(shape: &'static Shape) -> String {
    format!(
        "Descriptor(schema: {}, layout: {}, access: {})",
        schema_ref(shape),
        layout_expr(shape),
        access_expr(shape)
    )
}

fn access_expr(shape: &'static Shape) -> String {
    if is_bytes(shape) {
        return ".bytes(BytesAccess(stride: 1, elemAlign: 1, witness: .byteArray))".to_string();
    }
    match classify_shape(shape) {
        ShapeKind::Scalar(ScalarType::String | ScalarType::Str | ScalarType::CowStr) => {
            ".bytes(BytesAccess(stride: 1, elemAlign: 1, witness: .string))".to_string()
        }
        ShapeKind::Scalar(_) => ".scalar".to_string(),
        ShapeKind::Option { inner } => {
            let inner_ty = swift_type_base(inner);
            format!(
                ".option(OptionAccess(witness: .of({inner_ty}.self), some: {}))",
                descriptor_expr(inner)
            )
        }
        ShapeKind::List { element }
        | ShapeKind::Slice { element }
        | ShapeKind::Array { element, .. }
        | ShapeKind::Set { element } => sequence_or_bulk(element),
        ShapeKind::Map { key, value } => {
            assert!(
                matches!(
                    classify_shape(key),
                    ShapeKind::Scalar(ScalarType::String | ScalarType::Str | ScalarType::CowStr)
                ),
                "phon maps must be string-keyed"
            );
            let v_ty = swift_type_base(value);
            format!(
                ".map(MapAccess(key: {}, value: {}, keyStride: MemoryLayout<String>.stride, keyAlign: MemoryLayout<String>.alignment, valueStride: MemoryLayout<{v_ty}>.stride, valueAlign: MemoryLayout<{v_ty}>.alignment, witness: .stringKeyed({v_ty}.self)))",
                descriptor_expr(key),
                descriptor_expr(value)
            )
        }
        ShapeKind::Struct(StructInfo {
            fields,
            name: Some(type_name),
            ..
        }) => {
            let entries: Vec<String> = fields
                .iter()
                .map(|f| {
                    format!(
                        "FieldAccess(offset: MemoryLayout<{type_name}>.offset(of: \\{type_name}.{})!, descriptor: {})",
                        swift_field_name(f.name),
                        descriptor_expr(f.shape())
                    )
                })
                .collect();
            format!(
                ".record(RecordAccess(fields: [{}], construct: .inPlace))",
                entries.join(", ")
            )
        }
        ShapeKind::Enum(EnumInfo {
            name: Some(_),
            variants,
        }) => enum_access(shape, variants),
        ShapeKind::Result { ok, err } => result_access(ok, err),
        // An opaque field (the already-phon-encoded method payload) rides the wire
        // as a length-prefixed byte run — a `Primitive::Bytes`. In memory it is a
        // `[UInt8]`.
        ShapeKind::Opaque => {
            ".bytes(BytesAccess(stride: 1, elemAlign: 1, witness: .byteArray))".to_string()
        }
        other => panic!("phon descriptor: unsupported shape {other:?}"),
    }
}

/// A `[T]`: bulk byte run when `T` is a fixed scalar, else a per-element sequence.
fn sequence_or_bulk(element: &'static Shape) -> String {
    let ty = swift_type_base(element);
    if is_fixed_scalar(element) {
        format!(
            ".bytes(BytesAccess(stride: MemoryLayout<{ty}>.stride, elemAlign: MemoryLayout<{ty}>.alignment, witness: .scalarArray({ty}.self)))"
        )
    } else {
        format!(
            ".sequence(SequenceAccess(element: {}, stride: MemoryLayout<{ty}>.stride, elemAlign: MemoryLayout<{ty}>.alignment, witness: .of({ty}.self)))",
            descriptor_expr(element)
        )
    }
}

/// One unit/newtype variant's Swift case name + (for newtype) its payload shape.
fn variant_newtype_payload(variant: &facet_core::Variant) -> Option<&'static Shape> {
    match classify_variant(variant) {
        VariantKind::Unit => None,
        VariantKind::Newtype { .. } => Some(variant.data.fields[0].shape()),
        VariantKind::Tuple { .. } | VariantKind::Struct { .. } => {
            panic!("phon descriptor: tuple/struct enum variants not yet supported")
        }
    }
}

fn enum_access(shape: &'static Shape, variants: &[facet_core::Variant]) -> String {
    let ty = swift_type_base(shape);
    let mut tag_cases = String::new();
    let mut project_cases = String::new();
    let mut destroy_cases = String::new();
    let mut inject_cases = String::new();
    let mut variant_entries = Vec::new();

    for (i, v) in variants.iter().enumerate() {
        let case = swift_field_name(v.name);
        tag_cases.push_str(&format!("case .{case}: return {i}\n            "));
        match variant_newtype_payload(v) {
            None => {
                project_cases.push_str(&format!("case .{case}: break\n            "));
                inject_cases.push_str(&format!("case {i}: v = .{case}\n            "));
                variant_entries.push(format!(
                    "VariantAccess(wireIndex: {i}, payloadFields: [], payloadLayout: Layout(size: 0, align: 1))"
                ));
            }
            Some(payload) => {
                let pty = swift_type_base(payload);
                project_cases.push_str(&format!(
                    "case .{case}(let f0): scratch.assumingMemoryBound(to: {pty}.self).initialize(to: f0)\n            "
                ));
                destroy_cases.push_str(&format!(
                    "case {i}: scratch.assumingMemoryBound(to: {pty}.self).deinitialize(count: 1)\n            "
                ));
                inject_cases.push_str(&format!(
                    "case {i}: v = .{case}(scratch.assumingMemoryBound(to: {pty}.self).move())\n            "
                ));
                variant_entries.push(format!(
                    "VariantAccess(wireIndex: {i}, payloadFields: [FieldAccess(offset: 0, descriptor: {})], payloadLayout: MemoryLayout<{pty}>.phonLayout)",
                    descriptor_expr(payload)
                ));
            }
        }
    }

    format!(
        ".enumeration(EnumAccess(\n            tag: {{ ptr in switch ptr.assumingMemoryBound(to: {ty}.self).pointee {{\n            {tag_cases}}} }},\n            projectPayload: {{ value, _, scratch in switch value.assumingMemoryBound(to: {ty}.self).pointee {{\n            {project_cases}}} }},\n            destroyPayload: {{ scratch, localIndex in switch localIndex {{\n            {destroy_cases}default: break\n            }} }},\n            inject: {{ slot, localIndex, scratch in\n            let v: {ty}\n            switch localIndex {{\n            {inject_cases}default: fatalError(\"bad variant index\")\n            }}\n            slot.assumingMemoryBound(to: {ty}.self).initialize(to: v) }},\n            variants: [{}]))",
        variant_entries.join(", ")
    )
}

/// `Result<Ok, Err>` as a 2-variant enum (`.success`=0, `.failure`=1).
fn result_access(ok: &'static Shape, err: &'static Shape) -> String {
    let ok_ty = swift_type_base(ok);
    let err_ty = swift_type_base(err);
    let res_ty = format!("Result<{ok_ty}, {err_ty}>");
    format!(
        ".enumeration(EnumAccess(\n            tag: {{ ptr in switch ptr.assumingMemoryBound(to: {res_ty}.self).pointee {{ case .success: return 0; case .failure: return 1 }} }},\n            projectPayload: {{ value, _, scratch in switch value.assumingMemoryBound(to: {res_ty}.self).pointee {{ case .success(let f0): scratch.assumingMemoryBound(to: {ok_ty}.self).initialize(to: f0); case .failure(let f0): scratch.assumingMemoryBound(to: {err_ty}.self).initialize(to: f0) }} }},\n            destroyPayload: {{ scratch, localIndex in if localIndex == 0 {{ scratch.assumingMemoryBound(to: {ok_ty}.self).deinitialize(count: 1) }} else {{ scratch.assumingMemoryBound(to: {err_ty}.self).deinitialize(count: 1) }} }},\n            inject: {{ slot, localIndex, scratch in\n            let v: {res_ty} = localIndex == 0 ? .success(scratch.assumingMemoryBound(to: {ok_ty}.self).move()) : .failure(scratch.assumingMemoryBound(to: {err_ty}.self).move())\n            slot.assumingMemoryBound(to: {res_ty}.self).initialize(to: v) }},\n            variants: [VariantAccess(wireIndex: 0, payloadFields: [FieldAccess(offset: 0, descriptor: {})], payloadLayout: MemoryLayout<{ok_ty}>.phonLayout), VariantAccess(wireIndex: 1, payloadFields: [FieldAccess(offset: 0, descriptor: {})], payloadLayout: MemoryLayout<{err_ty}>.phonLayout)]))",
        descriptor_expr(ok),
        descriptor_expr(err)
    )
}
