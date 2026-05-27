use std::collections::HashSet;
use std::sync::Arc;

use facet::Facet;
use vox_types::schema::{self as vox_schema, PlanCacheKey, SchemaRecvTracker};
use vox_types::{BindingDirection, MethodId};

#[derive(Debug)]
pub enum SchemaDeserializeError {
    Protocol(String),
    Plan(String),
    Decode(String),
}

impl std::fmt::Display for SchemaDeserializeError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Protocol(message) => write!(f, "protocol error: {message}"),
            Self::Plan(message) => write!(f, "translation plan failed: {message}"),
            Self::Decode(message) => write!(f, "binette decode error: {message}"),
        }
    }
}

impl std::error::Error for SchemaDeserializeError {}

/// Deserialize args from a request (caller -> callee direction).
// r[impl schema.exchange.required]
pub fn schema_deserialize_args_borrowed<T: Facet<'static>>(
    bytes: &[u8],
    method_id: MethodId,
    tracker: &SchemaRecvTracker,
) -> Result<T, SchemaDeserializeError> {
    schema_deserialize_with_direction(bytes, method_id, BindingDirection::Args, tracker)
}

/// Deserialize a response (callee -> caller direction), borrowed-named variant.
// r[impl schema.exchange.required]
pub fn schema_deserialize_response_borrowed<T: Facet<'static>>(
    bytes: &[u8],
    method_id: MethodId,
    tracker: &SchemaRecvTracker,
) -> Result<T, SchemaDeserializeError> {
    schema_deserialize_with_direction(bytes, method_id, BindingDirection::Response, tracker)
}

/// Deserialize a response (callee -> caller direction), owned variant.
// r[impl schema.exchange.required]
pub fn schema_deserialize_response<T: Facet<'static>>(
    bytes: &[u8],
    method_id: MethodId,
    tracker: &SchemaRecvTracker,
) -> Result<T, SchemaDeserializeError> {
    schema_deserialize_with_direction(bytes, method_id, BindingDirection::Response, tracker)
}

// r[impl schema.exchange.required]
pub fn binette_schema_bundle_for_remote_binding(
    method_id: MethodId,
    direction: BindingDirection,
    tracker: &SchemaRecvTracker,
) -> Result<binette::SchemaBundle, SchemaDeserializeError> {
    let (remote_root, registry) = require_remote_vox_binding(method_id, direction, tracker)?;
    binette_schema_bundle_from_vox_schemas(remote_root, registry)
}

// r[impl schema.exchange.required]
pub fn encode_binette_schema_bundle_for_remote_binding(
    method_id: MethodId,
    direction: BindingDirection,
    tracker: &SchemaRecvTracker,
) -> Result<Vec<u8>, SchemaDeserializeError> {
    let bundle = binette_schema_bundle_for_remote_binding(method_id, direction, tracker)?;
    binette::encode_schema_bundle_to_vec(&bundle)
        .map_err(|error| SchemaDeserializeError::Plan(error.to_string()))
}

fn schema_deserialize_with_direction<T: Facet<'static>>(
    bytes: &[u8],
    method_id: MethodId,
    direction: BindingDirection,
    tracker: &SchemaRecvTracker,
) -> Result<T, SchemaDeserializeError> {
    let resolved = resolve_plan::<T>(method_id, direction, tracker)?;
    binette::decode_from_slice_with_plan(bytes, &resolved.plan, &resolved.registry)
        .map_err(|error| SchemaDeserializeError::Decode(error.to_string()))
}

struct ResolvedPlan {
    plan: binette::ReaderPlan,
    registry: binette::SchemaRegistry,
}

fn resolve_plan<T: Facet<'static>>(
    method_id: MethodId,
    direction: BindingDirection,
    tracker: &SchemaRecvTracker,
) -> Result<Arc<ResolvedPlan>, SchemaDeserializeError> {
    let cache_key = PlanCacheKey {
        method_id,
        direction,
        local_shape: T::SHAPE,
    };

    if let Some(cached) = tracker.get_cached_plan::<ResolvedPlan>(&cache_key) {
        return Ok(cached);
    }

    let resolved = Arc::new(build_resolved_plan::<T>(method_id, direction, tracker)?);
    tracker.insert_cached_plan(cache_key, Arc::clone(&resolved));
    Ok(resolved)
}

fn build_resolved_plan<T: Facet<'static>>(
    method_id: MethodId,
    direction: BindingDirection,
    tracker: &SchemaRecvTracker,
) -> Result<ResolvedPlan, SchemaDeserializeError> {
    let (remote_root, registry) = require_remote_binding(method_id, direction, tracker)?;

    let plan = binette::reader_plan_for::<T>(&remote_root, &registry)
        .map_err(|error| SchemaDeserializeError::Plan(error.to_string()))?;

    Ok(ResolvedPlan { plan, registry })
}

fn require_remote_binding(
    method_id: MethodId,
    direction: BindingDirection,
    tracker: &SchemaRecvTracker,
) -> Result<(binette::TypeRef, binette::SchemaRegistry), SchemaDeserializeError> {
    let (remote_root_ref, registry) = require_remote_vox_binding(method_id, direction, tracker)?;
    let bundle = binette_schema_bundle_from_vox_schemas(remote_root_ref, registry)?;
    let mut registry = binette::SchemaRegistry::new();
    registry
        .install_bundle(&bundle)
        .map_err(|error| SchemaDeserializeError::Plan(error.to_string()))?;
    Ok((bundle.root, registry))
}

fn require_remote_vox_binding(
    method_id: MethodId,
    direction: BindingDirection,
    tracker: &SchemaRecvTracker,
) -> Result<(vox_schema::TypeRef, vox_schema::SchemaRegistry), SchemaDeserializeError> {
    let dir_name = match direction {
        BindingDirection::Args => "args",
        BindingDirection::Response => "response",
    };

    let remote_root_ref = match direction {
        BindingDirection::Args => tracker.get_remote_args_root(method_id),
        BindingDirection::Response => tracker.get_remote_response_root(method_id),
    }
    .ok_or_else(|| {
        SchemaDeserializeError::Protocol(format!(
            "no remote {dir_name} schema received for method {method_id:?}; sender must send schemas before data"
        ))
    })?;

    let registry = tracker.received_registry();
    remote_root_ref.resolve_kind(&registry).ok_or_else(|| {
        SchemaDeserializeError::Protocol(format!(
            "remote root type ref {remote_root_ref:?} not found in received schemas"
        ))
    })?;
    Ok((remote_root_ref, registry))
}

fn binette_schema_bundle_from_vox_schemas(
    root: vox_schema::TypeRef,
    registry: vox_schema::SchemaRegistry,
) -> Result<binette::SchemaBundle, SchemaDeserializeError> {
    let argless_schema_ids = registry
        .values()
        .filter_map(|schema| {
            matches!(
                schema.kind,
                vox_schema::SchemaKind::Primitive { .. } | vox_schema::SchemaKind::Channel { .. }
            )
            .then_some(schema.id)
        })
        .collect::<HashSet<_>>();

    let bundle = binette::SchemaBundle {
        schemas: registry
            .into_values()
            .map(|schema| binette_schema_from_vox_schema(schema, &argless_schema_ids))
            .collect::<Result<_, _>>()?,
        root: binette_type_ref_from_vox_type_ref(root, &argless_schema_ids),
        attachments: Vec::new(),
    };
    binette::canonicalize_schema_bundle(bundle)
        .map_err(|error| SchemaDeserializeError::Plan(error.to_string()))
}

fn binette_schema_from_vox_schema(
    schema: vox_schema::Schema,
    argless_schema_ids: &HashSet<vox_schema::SchemaHash>,
) -> Result<binette::Schema, SchemaDeserializeError> {
    let type_params = if matches!(schema.kind, vox_schema::SchemaKind::Channel { .. }) {
        Vec::new()
    } else {
        schema
            .type_params
            .iter()
            .map(|param| param.0.clone())
            .collect()
    };

    Ok(binette::Schema {
        id: binette::TypeId(schema.id.0),
        type_params,
        kind: binette_schema_kind_from_vox_schema_kind(schema.kind, argless_schema_ids)?,
    })
}

fn binette_schema_kind_from_vox_schema_kind(
    kind: vox_schema::SchemaKind,
    argless_schema_ids: &HashSet<vox_schema::SchemaHash>,
) -> Result<binette::SchemaKind, SchemaDeserializeError> {
    Ok(match kind {
        vox_schema::SchemaKind::Struct { name, fields } => binette::SchemaKind::Struct {
            name,
            fields: fields
                .into_iter()
                .map(|field| binette::Field {
                    name: field.name,
                    type_ref: binette_type_ref_from_vox_type_ref(
                        field.type_ref,
                        argless_schema_ids,
                    ),
                    required: field.required,
                })
                .collect(),
        },
        vox_schema::SchemaKind::Enum { name, variants } => binette::SchemaKind::Enum {
            name,
            variants: variants
                .into_iter()
                .map(|variant| binette::Variant {
                    name: variant.name,
                    index: variant.index,
                    payload: binette_variant_payload_from_vox_variant_payload(
                        variant.payload,
                        argless_schema_ids,
                    ),
                })
                .collect(),
        },
        vox_schema::SchemaKind::Tuple { elements } => binette::SchemaKind::Tuple {
            elements: elements
                .into_iter()
                .map(|type_ref| binette_type_ref_from_vox_type_ref(type_ref, argless_schema_ids))
                .collect(),
        },
        vox_schema::SchemaKind::List { element } => binette::SchemaKind::List {
            element: binette_type_ref_from_vox_type_ref(element, argless_schema_ids),
        },
        vox_schema::SchemaKind::Map { key, value } => binette::SchemaKind::Map {
            key: binette_type_ref_from_vox_type_ref(key, argless_schema_ids),
            value: binette_type_ref_from_vox_type_ref(value, argless_schema_ids),
        },
        vox_schema::SchemaKind::Array { element, length } => binette::SchemaKind::Array {
            element: binette_type_ref_from_vox_type_ref(element, argless_schema_ids),
            dimensions: vec![length],
        },
        vox_schema::SchemaKind::Option { element } => binette::SchemaKind::Option {
            element: binette_type_ref_from_vox_type_ref(element, argless_schema_ids),
        },
        vox_schema::SchemaKind::Channel { .. } => {
            binette::SchemaKind::Primitive(binette::Primitive::Unit)
        }
        vox_schema::SchemaKind::Primitive { primitive_type } => {
            binette::SchemaKind::Primitive(binette_primitive_from_vox_primitive(primitive_type))
        }
    })
}

fn binette_variant_payload_from_vox_variant_payload(
    payload: vox_schema::VariantPayload,
    argless_schema_ids: &HashSet<vox_schema::SchemaHash>,
) -> binette::VariantPayload {
    match payload {
        vox_schema::VariantPayload::Unit => binette::VariantPayload::Unit,
        vox_schema::VariantPayload::Newtype { type_ref } => binette::VariantPayload::Newtype {
            type_ref: binette_type_ref_from_vox_type_ref(type_ref, argless_schema_ids),
        },
        vox_schema::VariantPayload::Tuple { types } => binette::VariantPayload::Tuple {
            elements: types
                .into_iter()
                .map(|type_ref| binette_type_ref_from_vox_type_ref(type_ref, argless_schema_ids))
                .collect(),
        },
        vox_schema::VariantPayload::Struct { fields } => binette::VariantPayload::Struct {
            fields: fields
                .into_iter()
                .map(|field| binette::Field {
                    name: field.name,
                    type_ref: binette_type_ref_from_vox_type_ref(
                        field.type_ref,
                        argless_schema_ids,
                    ),
                    required: field.required,
                })
                .collect(),
        },
    }
}

fn binette_type_ref_from_vox_type_ref(
    type_ref: vox_schema::TypeRef,
    argless_schema_ids: &HashSet<vox_schema::SchemaHash>,
) -> binette::TypeRef {
    match type_ref {
        vox_schema::TypeRef::Concrete { type_id, args } => binette::TypeRef::Concrete {
            type_id: binette::TypeId(type_id.0),
            args: if argless_schema_ids.contains(&type_id) {
                Vec::new()
            } else {
                args.into_iter()
                    .map(|type_ref| {
                        binette_type_ref_from_vox_type_ref(type_ref, argless_schema_ids)
                    })
                    .collect()
            },
        },
        vox_schema::TypeRef::Var { name } => binette::TypeRef::Var { name: name.0 },
    }
}

fn binette_primitive_from_vox_primitive(
    primitive: vox_schema::PrimitiveType,
) -> binette::Primitive {
    match primitive {
        vox_schema::PrimitiveType::Bool => binette::Primitive::Bool,
        vox_schema::PrimitiveType::U8 => binette::Primitive::U8,
        vox_schema::PrimitiveType::U16 => binette::Primitive::U16,
        vox_schema::PrimitiveType::U32 => binette::Primitive::U32,
        vox_schema::PrimitiveType::U64 => binette::Primitive::U64,
        vox_schema::PrimitiveType::U128 => binette::Primitive::U128,
        vox_schema::PrimitiveType::I8 => binette::Primitive::I8,
        vox_schema::PrimitiveType::I16 => binette::Primitive::I16,
        vox_schema::PrimitiveType::I32 => binette::Primitive::I32,
        vox_schema::PrimitiveType::I64 => binette::Primitive::I64,
        vox_schema::PrimitiveType::I128 => binette::Primitive::I128,
        vox_schema::PrimitiveType::F32 => binette::Primitive::F32,
        vox_schema::PrimitiveType::F64 => binette::Primitive::F64,
        vox_schema::PrimitiveType::Char => binette::Primitive::Char,
        vox_schema::PrimitiveType::String => binette::Primitive::String,
        vox_schema::PrimitiveType::Unit => binette::Primitive::Unit,
        vox_schema::PrimitiveType::Never => binette::Primitive::Never,
        vox_schema::PrimitiveType::Bytes => binette::Primitive::Bytes,
        vox_schema::PrimitiveType::Payload => binette::Primitive::Payload,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use vox_types::{SchemaPayload, extract_schemas};

    #[test]
    fn schema_deserialize_args_handles_nested_unary_tuple() {
        let method_id = MethodId(1);
        let extracted =
            extract_schemas(<((i32, String),) as Facet>::SHAPE).expect("schema extraction");
        let tracker = SchemaRecvTracker::new();
        tracker
            .record_received(
                method_id,
                BindingDirection::Args,
                SchemaPayload {
                    schemas: extracted.schemas.clone(),
                    root: extracted.root.clone(),
                },
            )
            .expect("record received schemas");

        let bytes =
            binette::encode_to_vec(&((42i32, "hello".to_string()),)).expect("serialize tuple args");
        let decoded: ((i32, String),) =
            schema_deserialize_args_borrowed(&bytes, method_id, &tracker)
                .expect("schema deserialize args");

        assert_eq!(decoded, ((42, "hello".to_string()),));
    }

    #[test]
    fn schema_deserialize_response_handles_tuple_result() {
        type Response = Result<(String, i32), vox_types::VoxError<core::convert::Infallible>>;

        let method_id = MethodId(2);
        let extracted = extract_schemas(<Response as Facet>::SHAPE).expect("schema extraction");
        let tracker = SchemaRecvTracker::new();
        tracker
            .record_received(
                method_id,
                BindingDirection::Response,
                SchemaPayload {
                    schemas: extracted.schemas.clone(),
                    root: extracted.root.clone(),
                },
            )
            .expect("record received schemas");

        let bytes = binette::encode_to_vec(
            &Ok::<_, vox_types::VoxError<core::convert::Infallible>>(("hello".to_string(), 42)),
        )
        .expect("serialize response");
        let decoded: Response = schema_deserialize_response_borrowed(&bytes, method_id, &tracker)
            .expect("schema deserialize response");

        let decoded = match decoded {
            Ok(decoded) => decoded,
            Err(error) => panic!("expected Ok response, got {error:?}"),
        };
        assert_eq!(decoded, ("hello".to_string(), 42));
    }
}
