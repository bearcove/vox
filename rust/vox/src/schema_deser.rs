use std::collections::HashSet;
use std::sync::Arc;

use facet::Facet;
use vox_types::schema::{self as vox_schema, PlanCacheKey, SchemaRecvTracker};
use vox_types::{BindingDirection, MethodId, SchemaPayloadBytes};

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

// r[impl schema.exchange.required]
pub fn vox_schema_payload_from_binette_schema_bundle(
    bundle: binette::SchemaBundle,
) -> Result<vox_schema::SchemaPayload, SchemaDeserializeError> {
    Ok(vox_schema::SchemaPayload {
        schemas: bundle
            .schemas
            .into_iter()
            .map(vox_schema_from_binette_schema)
            .collect::<Result<_, _>>()?,
        root: vox_type_ref_from_binette_type_ref(bundle.root),
    })
}

// r[impl schema.exchange.required]
pub fn encode_vox_schema_payload_from_binette_schema_bundle(
    bundle: binette::SchemaBundle,
) -> Result<SchemaPayloadBytes, SchemaDeserializeError> {
    Ok(vox_schema_payload_from_binette_schema_bundle(bundle)?.to_binette())
}

// r[impl schema.exchange.required]
pub fn encode_vox_schema_payload_from_binette_schema_bundle_bytes(
    bytes: &[u8],
) -> Result<SchemaPayloadBytes, SchemaDeserializeError> {
    let bundle = binette::decode_schema_bundle_from_slice(bytes)
        .map_err(|error| SchemaDeserializeError::Decode(error.to_string()))?;
    encode_vox_schema_payload_from_binette_schema_bundle(bundle)
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

fn vox_schema_from_binette_schema(
    schema: binette::Schema,
) -> Result<vox_schema::Schema, SchemaDeserializeError> {
    Ok(vox_schema::Schema {
        id: vox_schema::SchemaHash(schema.id.0),
        type_params: schema
            .type_params
            .into_iter()
            .map(vox_schema::TypeParamName)
            .collect(),
        kind: vox_schema_kind_from_binette_schema_kind(schema.kind)?,
    })
}

fn vox_schema_kind_from_binette_schema_kind(
    kind: binette::SchemaKind,
) -> Result<vox_schema::SchemaKind, SchemaDeserializeError> {
    Ok(match kind {
        binette::SchemaKind::Primitive(primitive) => vox_schema::SchemaKind::Primitive {
            primitive_type: vox_primitive_from_binette_primitive(primitive),
        },
        binette::SchemaKind::Struct { name, fields } => vox_schema::SchemaKind::Struct {
            name,
            fields: fields
                .into_iter()
                .map(|field| vox_schema::FieldSchema {
                    name: field.name,
                    type_ref: vox_type_ref_from_binette_type_ref(field.type_ref),
                    required: field.required,
                })
                .collect(),
        },
        binette::SchemaKind::Enum { name, variants } => vox_schema::SchemaKind::Enum {
            name,
            variants: variants
                .into_iter()
                .map(|variant| vox_schema::VariantSchema {
                    name: variant.name,
                    index: variant.index,
                    payload: vox_variant_payload_from_binette_variant_payload(variant.payload),
                })
                .collect(),
        },
        binette::SchemaKind::Tuple { elements } => vox_schema::SchemaKind::Tuple {
            elements: elements
                .into_iter()
                .map(vox_type_ref_from_binette_type_ref)
                .collect(),
        },
        binette::SchemaKind::List { element } => vox_schema::SchemaKind::List {
            element: vox_type_ref_from_binette_type_ref(element),
        },
        binette::SchemaKind::Set { .. } => {
            return Err(SchemaDeserializeError::Plan(
                "Vox schema model does not have a set kind".to_string(),
            ));
        }
        binette::SchemaKind::Map { key, value } => vox_schema::SchemaKind::Map {
            key: vox_type_ref_from_binette_type_ref(key),
            value: vox_type_ref_from_binette_type_ref(value),
        },
        binette::SchemaKind::Array {
            element,
            dimensions,
        } => {
            let [length] = dimensions.as_slice() else {
                return Err(SchemaDeserializeError::Plan(
                    "Vox schema model only supports one-dimensional arrays".to_string(),
                ));
            };
            vox_schema::SchemaKind::Array {
                element: vox_type_ref_from_binette_type_ref(element),
                length: *length,
            }
        }
        binette::SchemaKind::Option { element } => vox_schema::SchemaKind::Option {
            element: vox_type_ref_from_binette_type_ref(element),
        },
        binette::SchemaKind::Dynamic => {
            return Err(SchemaDeserializeError::Plan(
                "Vox schema model does not have a dynamic value kind".to_string(),
            ));
        }
        binette::SchemaKind::External { kind, metadata } => {
            vox_channel_schema_from_binette_external(kind, metadata)?
        }
    })
}

fn vox_channel_schema_from_binette_external(
    kind: String,
    metadata: binette::Value,
) -> Result<vox_schema::SchemaKind, SchemaDeserializeError> {
    if kind != "vox.channel" {
        return Err(SchemaDeserializeError::Plan(format!(
            "binette external attachment {kind:?} is not a Vox channel"
        )));
    }

    let binette::Value::Struct(fields) = metadata else {
        return Err(SchemaDeserializeError::Plan(
            "vox.channel external metadata must be a struct".to_owned(),
        ));
    };
    let direction = metadata_field(&fields, "direction")?;
    let element = metadata_field(&fields, "element")?;

    let direction = match direction {
        binette::Value::String(direction) if direction == "tx" => vox_schema::ChannelDirection::Tx,
        binette::Value::String(direction) if direction == "rx" => vox_schema::ChannelDirection::Rx,
        binette::Value::String(direction) => {
            return Err(SchemaDeserializeError::Plan(format!(
                "vox.channel direction must be \"tx\" or \"rx\", got {direction:?}"
            )));
        }
        other => {
            return Err(SchemaDeserializeError::Plan(format!(
                "vox.channel direction must be a string, got {other:?}"
            )));
        }
    };
    let element = binette::type_ref_from_value(element)
        .map(vox_type_ref_from_binette_type_ref)
        .map_err(|error| SchemaDeserializeError::Plan(error.to_string()))?;

    Ok(vox_schema::SchemaKind::Channel { direction, element })
}

fn metadata_field<'a>(
    fields: &'a [binette::FieldValue],
    name: &str,
) -> Result<&'a binette::Value, SchemaDeserializeError> {
    let mut matches = fields
        .iter()
        .filter(|field| field.name == name)
        .map(|field| &field.value);
    let Some(value) = matches.next() else {
        return Err(SchemaDeserializeError::Plan(format!(
            "vox.channel external metadata missing {name:?}"
        )));
    };
    if matches.next().is_some() {
        return Err(SchemaDeserializeError::Plan(format!(
            "vox.channel external metadata has duplicate {name:?}"
        )));
    }
    Ok(value)
}

fn vox_variant_payload_from_binette_variant_payload(
    payload: binette::VariantPayload,
) -> vox_schema::VariantPayload {
    match payload {
        binette::VariantPayload::Unit => vox_schema::VariantPayload::Unit,
        binette::VariantPayload::Newtype { type_ref } => vox_schema::VariantPayload::Newtype {
            type_ref: vox_type_ref_from_binette_type_ref(type_ref),
        },
        binette::VariantPayload::Tuple { elements } => vox_schema::VariantPayload::Tuple {
            types: elements
                .into_iter()
                .map(vox_type_ref_from_binette_type_ref)
                .collect(),
        },
        binette::VariantPayload::Struct { fields } => vox_schema::VariantPayload::Struct {
            fields: fields
                .into_iter()
                .map(|field| vox_schema::FieldSchema {
                    name: field.name,
                    type_ref: vox_type_ref_from_binette_type_ref(field.type_ref),
                    required: field.required,
                })
                .collect(),
        },
    }
}

fn vox_type_ref_from_binette_type_ref(type_ref: binette::TypeRef) -> vox_schema::TypeRef {
    match type_ref {
        binette::TypeRef::Concrete { type_id, args } => vox_schema::TypeRef::Concrete {
            type_id: vox_schema::SchemaHash(type_id.0),
            args: args
                .into_iter()
                .map(vox_type_ref_from_binette_type_ref)
                .collect(),
        },
        binette::TypeRef::Var { name } => vox_schema::TypeRef::Var {
            name: vox_schema::TypeParamName(name),
        },
    }
}

fn vox_primitive_from_binette_primitive(
    primitive: binette::Primitive,
) -> vox_schema::PrimitiveType {
    match primitive {
        binette::Primitive::Bool => vox_schema::PrimitiveType::Bool,
        binette::Primitive::U8 => vox_schema::PrimitiveType::U8,
        binette::Primitive::U16 => vox_schema::PrimitiveType::U16,
        binette::Primitive::U32 => vox_schema::PrimitiveType::U32,
        binette::Primitive::U64 => vox_schema::PrimitiveType::U64,
        binette::Primitive::U128 => vox_schema::PrimitiveType::U128,
        binette::Primitive::I8 => vox_schema::PrimitiveType::I8,
        binette::Primitive::I16 => vox_schema::PrimitiveType::I16,
        binette::Primitive::I32 => vox_schema::PrimitiveType::I32,
        binette::Primitive::I64 => vox_schema::PrimitiveType::I64,
        binette::Primitive::I128 => vox_schema::PrimitiveType::I128,
        binette::Primitive::F32 => vox_schema::PrimitiveType::F32,
        binette::Primitive::F64 => vox_schema::PrimitiveType::F64,
        binette::Primitive::Char => vox_schema::PrimitiveType::Char,
        binette::Primitive::String => vox_schema::PrimitiveType::String,
        binette::Primitive::Unit => vox_schema::PrimitiveType::Unit,
        binette::Primitive::Never => vox_schema::PrimitiveType::Never,
        binette::Primitive::Bytes => vox_schema::PrimitiveType::Bytes,
        binette::Primitive::Payload => vox_schema::PrimitiveType::Payload,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use vox_types::{SchemaPayload, Tx, extract_schemas};

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

    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    fn remote_vox_arg_schema_drives_binette_local_tuple_decode() {
        type Args = (String, u32);

        let method_id = MethodId(3);
        let extracted = extract_schemas(<Args as Facet>::SHAPE).expect("schema extraction");
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

        let writer_bundle =
            binette_schema_bundle_for_remote_binding(method_id, BindingDirection::Args, &tracker)
                .expect("Vox schemas should convert to a binette writer bundle");
        let mut writer_registry = binette::SchemaRegistry::new();
        writer_registry
            .install_bundle(&writer_bundle)
            .expect("writer bundle should install");

        let mut local_descriptor = binette::local_access::rust_facet_descriptor_for::<Args>()
            .expect("local tuple descriptor");
        let reader_bundle = binette::local_access::synthetic_schema_bundle_for_local_descriptor(
            &mut local_descriptor,
        )
        .expect("local tuple descriptor should synthesize a reader bundle");
        let reader_plan = binette::reader_plan_for_bundles(&writer_bundle, &reader_bundle)
            .expect("remote writer tuple should plan into local tuple descriptor");
        let decoder = binette::hybrid_local_stencil_decoder_from_plan(
            &reader_plan,
            &writer_registry,
            &local_descriptor,
            &binette::local_access::LocalThunkBindings::new(),
        )
        .expect("tuple method args should compile to local decode stencil");

        let bytes = binette::encode_to_vec(&("swift-bound args".to_string(), 0xCAFE_BABEu32))
            .expect("encode writer args");
        let mut out = std::mem::MaybeUninit::<Args>::uninit();
        unsafe {
            decoder
                .decode_raw_into(&bytes, out.as_mut_ptr().cast())
                .expect("local tuple decode should succeed");
        }
        let decoded = unsafe { out.assume_init() };

        assert_eq!(decoded, ("swift-bound args".to_string(), 0xCAFE_BABE));
    }

    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    fn binette_local_tuple_schema_bundle_can_be_advertised_as_vox_schema_payload() {
        type Args = (String, u32);

        let mut local_descriptor = binette::local_access::rust_facet_descriptor_for::<Args>()
            .expect("local tuple descriptor");
        let local_bundle = binette::local_access::synthetic_schema_bundle_for_local_descriptor(
            &mut local_descriptor,
        )
        .expect("local tuple descriptor should synthesize a schema bundle");
        let bundle_bytes = binette::encode_schema_bundle_to_vec(&local_bundle)
            .expect("encode local schema bundle");

        let schema_bytes =
            encode_vox_schema_payload_from_binette_schema_bundle_bytes(&bundle_bytes)
                .expect("convert binette schema bundle to Vox schema payload bytes");
        let payload =
            SchemaPayload::from_binette(&schema_bytes.0).expect("converted payload should parse");

        let method_id = MethodId(4);
        let tracker = SchemaRecvTracker::new();
        tracker
            .record_received(method_id, BindingDirection::Args, payload)
            .expect("converted payload should install as a remote Vox binding");

        let writer_bundle =
            binette_schema_bundle_for_remote_binding(method_id, BindingDirection::Args, &tracker)
                .expect("converted Vox binding should produce a binette writer bundle");
        let mut writer_registry = binette::SchemaRegistry::new();
        writer_registry
            .install_bundle(&writer_bundle)
            .expect("writer bundle should install");
        let reader_plan = binette::reader_plan_for_bundles(&writer_bundle, &local_bundle)
            .expect("converted writer bundle should plan back into the local bundle");
        let decoder = binette::hybrid_local_stencil_decoder_from_plan(
            &reader_plan,
            &writer_registry,
            &local_descriptor,
            &binette::local_access::LocalThunkBindings::new(),
        )
        .expect("converted writer bundle should drive local tuple decode");

        let bytes = binette::encode_to_vec(&("schema-advertised args".to_string(), 7u32))
            .expect("encode writer args");
        let mut out = std::mem::MaybeUninit::<Args>::uninit();
        unsafe {
            decoder
                .decode_raw_into(&bytes, out.as_mut_ptr().cast())
                .expect("local tuple decode should succeed");
        }
        let decoded = unsafe { out.assume_init() };

        assert_eq!(decoded, ("schema-advertised args".to_string(), 7));
    }

    #[test]
    fn binette_external_schema_requires_vox_specific_metadata_before_advertising() {
        let bundle = binette::SchemaBundle {
            schemas: vec![binette::Schema {
                id: binette::TypeId(0xB1_0000_0000_3000),
                type_params: Vec::new(),
                kind: binette::SchemaKind::External {
                    kind: "vox.channel".to_string(),
                    metadata: binette::Value::Unit,
                },
            }],
            root: binette::TypeRef::concrete(binette::TypeId(0xB1_0000_0000_3000)),
            attachments: Vec::new(),
        };

        let error = encode_vox_schema_payload_from_binette_schema_bundle(bundle)
            .expect_err("external attachments need a Vox channel metadata convention");

        assert!(
            matches!(error, SchemaDeserializeError::Plan(message) if message.contains("vox.channel"))
        );
    }

    #[test]
    fn binette_external_channel_schema_advertises_as_vox_channel() {
        let channel_id = binette::TypeId(0xB1_0000_0000_3001);
        let element =
            binette::TypeRef::concrete(binette::primitive_type_id(binette::Primitive::U32));
        let bundle = binette::SchemaBundle {
            schemas: vec![binette::Schema {
                id: channel_id,
                type_params: Vec::new(),
                kind: binette::SchemaKind::External {
                    kind: "vox.channel".to_string(),
                    metadata: binette::Value::Struct(vec![
                        binette::FieldValue {
                            name: "direction".to_owned(),
                            value: binette::Value::String("tx".to_owned()),
                        },
                        binette::FieldValue {
                            name: "element".to_owned(),
                            value: binette::type_ref_to_value(&element)
                                .expect("type ref metadata should encode"),
                        },
                    ]),
                },
            }],
            root: binette::TypeRef::concrete(channel_id),
            attachments: Vec::new(),
        };

        let schema_bytes = encode_vox_schema_payload_from_binette_schema_bundle(bundle)
            .expect("vox.channel metadata should become a Vox Channel schema");
        let payload =
            SchemaPayload::from_binette(&schema_bytes.0).expect("converted payload should parse");
        let channel = payload
            .schemas
            .iter()
            .find(|schema| schema.id == vox_schema::SchemaHash(channel_id.0))
            .expect("channel schema should be present");

        let vox_schema::SchemaKind::Channel { direction, element } = &channel.kind else {
            panic!("expected channel schema, got {channel:?}");
        };
        assert_eq!(*direction, vox_schema::ChannelDirection::Tx);
        assert_eq!(
            *element,
            vox_schema::TypeRef::concrete(vox_schema::SchemaHash(
                binette::primitive_type_id(binette::Primitive::U32).0
            ))
        );
    }

    #[test]
    fn vox_channel_schema_lowers_to_unit_for_binette_payloads() {
        let extracted = extract_schemas(<Tx<u32> as Facet>::SHAPE).expect("schema extraction");
        let registry = extracted
            .schemas
            .clone()
            .into_iter()
            .map(|schema| (schema.id, schema))
            .collect::<vox_schema::SchemaRegistry>();
        let bundle = binette_schema_bundle_from_vox_schemas(extracted.root.clone(), registry)
            .expect("Vox channel schemas should become binette payload schemas");
        let binette::TypeRef::Concrete { type_id, args } = &bundle.root else {
            panic!("expected concrete channel root, got {:#?}", bundle.root);
        };
        assert!(args.is_empty());
        let channel = bundle
            .schemas
            .iter()
            .find(|schema| schema.id == *type_id)
            .expect("channel schema should be present");

        assert_eq!(
            channel.kind,
            binette::SchemaKind::Primitive(binette::Primitive::Unit)
        );
    }
}
