use std::sync::Arc;

use facet::Facet;
use vox_types::schema::{PlanCacheKey, SchemaRecvTracker};
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
            Self::Plan(message) => write!(f, "binette plan error: {message}"),
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
    require_remote_binding(method_id, direction, tracker)?;

    let writer_plan = binette::writer_plan_for::<T>()
        .map_err(|error| SchemaDeserializeError::Plan(error.to_string()))?;
    let mut registry = binette::SchemaRegistry::new();
    registry
        .install_bundle(writer_plan.schema_bundle())
        .map_err(|error| SchemaDeserializeError::Plan(error.to_string()))?;
    let plan = binette::reader_plan_for::<T>(writer_plan.root(), &registry)
        .map_err(|error| SchemaDeserializeError::Plan(error.to_string()))?;

    Ok(ResolvedPlan { plan, registry })
}

fn require_remote_binding(
    method_id: MethodId,
    direction: BindingDirection,
    tracker: &SchemaRecvTracker,
) -> Result<(), SchemaDeserializeError> {
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
    Ok(())
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
