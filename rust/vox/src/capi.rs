use facet::Facet;

use crate::ReplySink as _;
use crate::schema_deser::{
    encode_binette_schema_bundle_from_vox_schema_payload_bytes,
    encode_vox_schema_payload_for_remote_binding,
    encode_vox_schema_payload_from_binette_schema_bundle_bytes,
};

pub const VOX_STATUS_OK: i32 = 0;
pub const VOX_STATUS_NULL_POINTER: i32 = 1;
pub const VOX_STATUS_SCHEMA: i32 = 2;

#[repr(C)]
pub struct VoxByteBuffer {
    pub ptr: *mut u8,
    pub len: usize,
    pub cap: usize,
}

impl VoxByteBuffer {
    fn empty() -> Self {
        Self {
            ptr: std::ptr::null_mut(),
            len: 0,
            cap: 0,
        }
    }

    fn from_vec(mut bytes: Vec<u8>) -> Self {
        let buffer = Self {
            ptr: bytes.as_mut_ptr(),
            len: bytes.len(),
            cap: bytes.capacity(),
        };
        std::mem::forget(bytes);
        buffer
    }
}

// r[impl schema.exchange.required]
#[unsafe(no_mangle)]
pub extern "C" fn vox_byte_buffer_free(buffer: VoxByteBuffer) {
    if !buffer.ptr.is_null() {
        drop(unsafe { Vec::from_raw_parts(buffer.ptr, buffer.len, buffer.cap) });
    }
}

// r[impl schema.exchange.required]
#[unsafe(no_mangle)]
pub extern "C" fn vox_schema_payload_from_binette_schema_bundle(
    schema_bundle_ptr: *const u8,
    schema_bundle_len: usize,
    out: *mut VoxByteBuffer,
) -> i32 {
    let Some(out) = (unsafe { out.as_mut() }) else {
        return VOX_STATUS_NULL_POINTER;
    };
    *out = VoxByteBuffer::empty();
    let Some(schema_bundle_ptr) = (unsafe { schema_bundle_ptr.as_ref() }) else {
        return VOX_STATUS_NULL_POINTER;
    };
    let schema_bundle = unsafe { std::slice::from_raw_parts(schema_bundle_ptr, schema_bundle_len) };
    match encode_vox_schema_payload_from_binette_schema_bundle_bytes(schema_bundle) {
        Ok(bytes) => {
            *out = VoxByteBuffer::from_vec(bytes.0);
            VOX_STATUS_OK
        }
        Err(_) => VOX_STATUS_SCHEMA,
    }
}

// r[impl schema.exchange.required]
#[unsafe(no_mangle)]
pub extern "C" fn vox_binette_schema_bundle_from_schema_payload(
    schema_payload_ptr: *const u8,
    schema_payload_len: usize,
    out: *mut VoxByteBuffer,
) -> i32 {
    let Some(out) = (unsafe { out.as_mut() }) else {
        return VOX_STATUS_NULL_POINTER;
    };
    *out = VoxByteBuffer::empty();
    let Some(schema_payload_ptr) = (unsafe { schema_payload_ptr.as_ref() }) else {
        return VOX_STATUS_NULL_POINTER;
    };
    let schema_payload =
        unsafe { std::slice::from_raw_parts(schema_payload_ptr, schema_payload_len) };
    match encode_binette_schema_bundle_from_vox_schema_payload_bytes(schema_payload) {
        Ok(bytes) => {
            *out = VoxByteBuffer::from_vec(bytes);
            VOX_STATUS_OK
        }
        Err(_) => VOX_STATUS_SCHEMA,
    }
}

// r[impl schema.exchange.required]
#[unsafe(no_mangle)]
pub extern "C" fn vox_canary_accept_swift_args(
    schema_payload_ptr: *const u8,
    schema_payload_len: usize,
    payload_ptr: *const u8,
    payload_len: usize,
) -> i32 {
    let Some(schema_payload_ptr) = (unsafe { schema_payload_ptr.as_ref() }) else {
        return VOX_STATUS_NULL_POINTER;
    };
    let Some(payload_ptr) = (unsafe { payload_ptr.as_ref() }) else {
        return VOX_STATUS_NULL_POINTER;
    };
    let schema_payload =
        unsafe { std::slice::from_raw_parts(schema_payload_ptr, schema_payload_len) };
    let payload = unsafe { std::slice::from_raw_parts(payload_ptr, payload_len) };

    match accept_swift_args(schema_payload, payload) {
        Ok(()) => VOX_STATUS_OK,
        Err(_) => VOX_STATUS_SCHEMA,
    }
}

// r[impl schema.exchange.required]
#[unsafe(no_mangle)]
pub extern "C" fn vox_canary_call_swift_args(
    schema_payload_ptr: *const u8,
    schema_payload_len: usize,
    payload_ptr: *const u8,
    payload_len: usize,
    response_schema_payload_out: *mut VoxByteBuffer,
    response_payload_out: *mut VoxByteBuffer,
) -> i32 {
    let Some(response_schema_payload_out) = (unsafe { response_schema_payload_out.as_mut() })
    else {
        return VOX_STATUS_NULL_POINTER;
    };
    *response_schema_payload_out = VoxByteBuffer::empty();
    let Some(response_payload_out) = (unsafe { response_payload_out.as_mut() }) else {
        return VOX_STATUS_NULL_POINTER;
    };
    *response_payload_out = VoxByteBuffer::empty();

    let Some(schema_payload_ptr) = (unsafe { schema_payload_ptr.as_ref() }) else {
        return VOX_STATUS_NULL_POINTER;
    };
    let Some(payload_ptr) = (unsafe { payload_ptr.as_ref() }) else {
        return VOX_STATUS_NULL_POINTER;
    };
    let schema_payload =
        unsafe { std::slice::from_raw_parts(schema_payload_ptr, schema_payload_len) };
    let payload = unsafe { std::slice::from_raw_parts(payload_ptr, payload_len) };

    match call_swift_args(schema_payload, payload) {
        Ok((response_schema_payload, response_payload)) => {
            *response_schema_payload_out = VoxByteBuffer::from_vec(response_schema_payload);
            *response_payload_out = VoxByteBuffer::from_vec(response_payload);
            VOX_STATUS_OK
        }
        Err(_) => VOX_STATUS_SCHEMA,
    }
}

// r[impl schema.exchange.required]
#[unsafe(no_mangle)]
pub extern "C" fn vox_canary_driver_call_swift_args(
    schema_payload_ptr: *const u8,
    schema_payload_len: usize,
    payload_ptr: *const u8,
    payload_len: usize,
    response_schema_payload_out: *mut VoxByteBuffer,
    response_payload_out: *mut VoxByteBuffer,
) -> i32 {
    let Some(response_schema_payload_out) = (unsafe { response_schema_payload_out.as_mut() })
    else {
        return VOX_STATUS_NULL_POINTER;
    };
    *response_schema_payload_out = VoxByteBuffer::empty();
    let Some(response_payload_out) = (unsafe { response_payload_out.as_mut() }) else {
        return VOX_STATUS_NULL_POINTER;
    };
    *response_payload_out = VoxByteBuffer::empty();

    let Some(schema_payload_ptr) = (unsafe { schema_payload_ptr.as_ref() }) else {
        return VOX_STATUS_NULL_POINTER;
    };
    let Some(payload_ptr) = (unsafe { payload_ptr.as_ref() }) else {
        return VOX_STATUS_NULL_POINTER;
    };
    let schema_payload =
        unsafe { std::slice::from_raw_parts(schema_payload_ptr, schema_payload_len) };
    let payload = unsafe { std::slice::from_raw_parts(payload_ptr, payload_len) };

    match driver_call_swift_args(schema_payload, payload) {
        Ok((response_schema_payload, response_payload)) => {
            *response_schema_payload_out = VoxByteBuffer::from_vec(response_schema_payload);
            *response_payload_out = VoxByteBuffer::from_vec(response_payload);
            VOX_STATUS_OK
        }
        Err(_) => VOX_STATUS_SCHEMA,
    }
}

// r[impl schema.exchange.required]
#[unsafe(no_mangle)]
pub extern "C" fn vox_canary_driver_call_swift_rich(
    schema_payload_ptr: *const u8,
    schema_payload_len: usize,
    payload_ptr: *const u8,
    payload_len: usize,
    response_schema_payload_out: *mut VoxByteBuffer,
    response_payload_out: *mut VoxByteBuffer,
) -> i32 {
    let Some(response_schema_payload_out) = (unsafe { response_schema_payload_out.as_mut() })
    else {
        return VOX_STATUS_NULL_POINTER;
    };
    *response_schema_payload_out = VoxByteBuffer::empty();
    let Some(response_payload_out) = (unsafe { response_payload_out.as_mut() }) else {
        return VOX_STATUS_NULL_POINTER;
    };
    *response_payload_out = VoxByteBuffer::empty();

    let Some(schema_payload_ptr) = (unsafe { schema_payload_ptr.as_ref() }) else {
        return VOX_STATUS_NULL_POINTER;
    };
    let Some(payload_ptr) = (unsafe { payload_ptr.as_ref() }) else {
        return VOX_STATUS_NULL_POINTER;
    };
    let schema_payload =
        unsafe { std::slice::from_raw_parts(schema_payload_ptr, schema_payload_len) };
    let payload = unsafe { std::slice::from_raw_parts(payload_ptr, payload_len) };

    match driver_call_swift_rich(schema_payload, payload) {
        Ok((response_schema_payload, response_payload)) => {
            *response_schema_payload_out = VoxByteBuffer::from_vec(response_schema_payload);
            *response_payload_out = VoxByteBuffer::from_vec(response_payload);
            VOX_STATUS_OK
        }
        Err(_) => VOX_STATUS_SCHEMA,
    }
}

fn accept_swift_args(schema_payload: &[u8], payload: &[u8]) -> Result<(), String> {
    type Args = (String, Option<u16>, ());

    let schema_payload = vox_types::SchemaPayload::from_binette(schema_payload)
        .map_err(|error| error.to_string())?;
    let method_id = vox_types::MethodId(0xB1_0000_0000_5000);
    let tracker = vox_types::SchemaRecvTracker::new();
    tracker
        .record_received(method_id, vox_types::BindingDirection::Args, schema_payload)
        .map_err(|error| error.to_string())?;

    let decoded: Args =
        crate::schema_deser::schema_deserialize_args_borrowed(payload, method_id, &tracker)
            .map_err(|error| error.to_string())?;

    if decoded == ("swift rpc args".to_owned(), Some(144), ()) {
        Ok(())
    } else {
        Err(format!("unexpected Swift args payload: {decoded:?}"))
    }
}

#[derive(Debug, Clone, PartialEq, Facet)]
struct SwiftCanaryReply {
    message: String,
    retry: Option<u16>,
}

#[derive(Debug, Clone, PartialEq, Facet)]
struct SwiftCanaryCall {
    method: u32,
    title: String,
    payload: Vec<u8>,
    retry: Option<u16>,
    outcome: SwiftCanaryOutcome,
    output: (),
}

#[allow(non_camel_case_types)]
#[derive(Debug, Clone, PartialEq, Facet)]
#[repr(u8)]
enum SwiftCanaryOutcome {
    accepted(String),
    rejected(u32),
}

fn call_swift_args(schema_payload: &[u8], payload: &[u8]) -> Result<(Vec<u8>, Vec<u8>), String> {
    accept_swift_args(schema_payload, payload)?;

    let reply = SwiftCanaryReply {
        message: "rust vox response".to_owned(),
        retry: Some(233),
    };
    let response_payload = binette::encode_to_vec(&reply).map_err(|error| error.to_string())?;
    let response_schema_payload =
        vox_types::SchemaSendTracker::plan_for_shape(SwiftCanaryReply::SHAPE)
            .map_err(|error| error.to_string())?
            .to_binette()
            .0;

    Ok((response_schema_payload, response_payload))
}

#[derive(Clone)]
struct SwiftCanaryDriverHandler;

#[derive(Clone)]
struct SwiftRichCanaryDriverHandler;

impl crate::Handler<crate::DriverReplySink> for SwiftCanaryDriverHandler {
    async fn handle(
        &self,
        call: crate::SelfRef<crate::RequestCall<'static>>,
        reply: crate::DriverReplySink,
        schemas: std::sync::Arc<vox_types::SchemaRecvTracker>,
    ) {
        let method_id = vox_types::MethodId(0xB1_0000_0000_5000);
        let call = call.get();
        let args = match &call.args {
            vox_types::Payload::BinetteBytes(bytes) => {
                crate::schema_deser::schema_deserialize_args_borrowed::<(String, Option<u16>, ())>(
                    bytes, method_id, &schemas,
                )
            }
            _ => Err(crate::schema_deser::SchemaDeserializeError::Protocol(
                "driver canary expected incoming binette bytes".to_owned(),
            )),
        };

        let reply_value = match args {
            Ok(args) if args == ("swift rpc args".to_owned(), Some(144), ()) => SwiftCanaryReply {
                message: "rust vox response".to_owned(),
                retry: Some(233),
            },
            Ok(other) => SwiftCanaryReply {
                message: format!("unexpected args: {other:?}"),
                retry: None,
            },
            Err(error) => SwiftCanaryReply {
                message: format!("decode error: {error}"),
                retry: None,
            },
        };

        reply
            .send_reply(crate::RequestResponse {
                ret: crate::Payload::outgoing(&reply_value),
                schemas: Default::default(),
                metadata: Default::default(),
            })
            .await;
    }
}

impl crate::Handler<crate::DriverReplySink> for SwiftRichCanaryDriverHandler {
    async fn handle(
        &self,
        call: crate::SelfRef<crate::RequestCall<'static>>,
        reply: crate::DriverReplySink,
        schemas: std::sync::Arc<vox_types::SchemaRecvTracker>,
    ) {
        let method_id = vox_types::MethodId(0xB1_0000_0000_7000);
        let call = call.get();
        let decoded = match &call.args {
            vox_types::Payload::BinetteBytes(bytes) => {
                crate::schema_deser::schema_deserialize_args_borrowed::<SwiftCanaryCall>(
                    bytes, method_id, &schemas,
                )
            }
            _ => Err(crate::schema_deser::SchemaDeserializeError::Protocol(
                "rich driver canary expected incoming binette bytes".to_owned(),
            )),
        };

        let reply_value = match decoded {
            Ok(call) => reply_for_swift_canary_call(call),
            Err(error) => SwiftCanaryReply {
                message: format!("decode error: {error}"),
                retry: None,
            },
        };

        reply
            .send_reply(crate::RequestResponse {
                ret: crate::Payload::outgoing(&reply_value),
                schemas: Default::default(),
                metadata: Default::default(),
            })
            .await;
    }
}

fn reply_for_swift_canary_call(call: SwiftCanaryCall) -> SwiftCanaryReply {
    let outcome = match call.outcome {
        SwiftCanaryOutcome::accepted(message) => format!("accepted:{message}"),
        SwiftCanaryOutcome::rejected(code) => format!("rejected:{code}"),
    };
    SwiftCanaryReply {
        message: format!(
            "{}:{}:{}:{}",
            call.method,
            call.title,
            call.payload.len(),
            outcome
        ),
        retry: call.retry,
    }
}

fn driver_call_swift_args(
    schema_payload: &[u8],
    payload: &[u8],
) -> Result<(Vec<u8>, Vec<u8>), String> {
    driver_call_with_handler(
        schema_payload,
        payload,
        vox_types::MethodId(0xB1_0000_0000_5000),
        SwiftCanaryDriverHandler,
    )
}

fn driver_call_swift_rich(
    schema_payload: &[u8],
    payload: &[u8],
) -> Result<(Vec<u8>, Vec<u8>), String> {
    driver_call_with_handler(
        schema_payload,
        payload,
        vox_types::MethodId(0xB1_0000_0000_7000),
        SwiftRichCanaryDriverHandler,
    )
}

async fn call_driver_once<H>(
    schema_payload: Vec<u8>,
    payload: Vec<u8>,
    method_id: vox_types::MethodId,
    handler: H,
) -> Result<vox_types::WithTracker<crate::SelfRef<crate::RequestResponse<'static>>>, String>
where
    H: crate::Handler<crate::DriverReplySink> + Clone + Send + Sync + 'static,
{
    let (client_link, server_link) = crate::memory_link_pair(16);
    let server_task = tokio::task::spawn(async move {
        crate::acceptor_on(server_link)
            .on_connection(handler)
            .establish::<crate::NoopClient>()
            .await
    });
    let caller = crate::initiator_on(client_link, crate::TransportMode::Bare)
        .establish::<crate::NoopClient>()
        .await
        .map_err(|error| error.to_string())?;
    let server_guard = server_task
        .await
        .map_err(|error| error.to_string())?
        .map_err(|error| error.to_string())?;

    let response = caller
        .caller
        .call(crate::RequestCall {
            method_id,
            args: crate::Payload::BinetteOwned(payload),
            channels: Vec::new(),
            schemas: vox_types::SchemaPayloadBytes(schema_payload),
            metadata: Default::default(),
        })
        .await
        .map_err(|error| error.to_string())?;

    drop(caller);
    drop(server_guard);
    Ok(response)
}

fn driver_call_with_handler<H>(
    schema_payload: &[u8],
    payload: &[u8],
    method_id: vox_types::MethodId,
    handler: H,
) -> Result<(Vec<u8>, Vec<u8>), String>
where
    H: crate::Handler<crate::DriverReplySink> + Clone + Send + Sync + 'static,
{
    let schema_payload = schema_payload.to_vec();
    let payload = payload.to_vec();
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .map_err(|error| error.to_string())?;

    runtime.block_on(async move {
        let response = call_driver_once(schema_payload, payload, method_id, handler).await?;
        let response_schema_payload = encode_vox_schema_payload_for_remote_binding(
            method_id,
            vox_types::BindingDirection::Response,
            &response.tracker,
        )
        .map_err(|error| error.to_string())?
        .0;
        let response_payload = match &response.get().ret {
            vox_types::Payload::BinetteBytes(bytes) => bytes.to_vec(),
            vox_types::Payload::BinetteOwned(bytes) => bytes.clone(),
            vox_types::Payload::Value { .. } => {
                return Err("driver canary expected response binette bytes".to_owned());
            }
        };

        Ok((response_schema_payload, response_payload))
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    // r[verify schema.exchange.required]
    #[test]
    fn c_api_converts_binette_schema_bundle_to_vox_schema_payload_bytes() {
        let bundle = binette::schema_bundle_for::<(String, u32)>()
            .expect("test schema bundle should extract");
        let bundle_bytes =
            binette::encode_schema_bundle_to_vec(&bundle).expect("schema bundle should encode");
        let mut out = VoxByteBuffer::empty();

        let status = vox_schema_payload_from_binette_schema_bundle(
            bundle_bytes.as_ptr(),
            bundle_bytes.len(),
            &mut out,
        );

        assert_eq!(status, VOX_STATUS_OK);
        assert!(!out.ptr.is_null());
        assert_ne!(out.len, 0);
        let payload_bytes = unsafe { std::slice::from_raw_parts(out.ptr, out.len) };
        let payload = vox_types::SchemaPayload::from_binette(payload_bytes)
            .expect("converted schema payload should parse");
        assert!(!payload.schemas.is_empty());
        vox_byte_buffer_free(out);
    }

    // r[verify schema.exchange.required]
    #[test]
    fn c_api_schema_payload_drives_vox_receive_deserialization() {
        type Args = (String, u32);

        let mut descriptor = binette::local_access::rust_facet_descriptor_for::<Args>()
            .expect("test descriptor should extract");
        let bundle =
            binette::local_access::synthetic_schema_bundle_for_local_descriptor(&mut descriptor)
                .expect("test descriptor schema should synthesize");
        let bundle_bytes =
            binette::encode_schema_bundle_to_vec(&bundle).expect("schema bundle should encode");
        let mut out = VoxByteBuffer::empty();
        let status = vox_schema_payload_from_binette_schema_bundle(
            bundle_bytes.as_ptr(),
            bundle_bytes.len(),
            &mut out,
        );
        assert_eq!(status, VOX_STATUS_OK);
        let payload_bytes = unsafe { std::slice::from_raw_parts(out.ptr, out.len) };
        let payload = vox_types::SchemaPayload::from_binette(payload_bytes)
            .expect("converted schema payload should parse");
        vox_byte_buffer_free(out);

        let method_id = vox_types::MethodId(0xB1_0000_0000_4000);
        let tracker = vox_types::SchemaRecvTracker::new();
        tracker
            .record_received(method_id, vox_types::BindingDirection::Args, payload)
            .expect("converted payload should install as received args schemas");
        let bytes = binette::encode_to_vec(&("swift-ish args".to_owned(), 0xCAFE_BABEu32))
            .expect("args should encode");

        let decoded: Args =
            crate::schema_deser::schema_deserialize_args_borrowed(&bytes, method_id, &tracker)
                .expect("received schema should drive normal Vox arg deserialization");

        assert_eq!(decoded, ("swift-ish args".to_owned(), 0xCAFE_BABE));
    }
}
