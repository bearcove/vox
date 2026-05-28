use crate::schema_deser::encode_vox_schema_payload_from_binette_schema_bundle_bytes;

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
