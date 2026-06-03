//! phon as a vox codec.
//!
//! This is the data-plane adapter for the codec migration: encode/decode a
//! `#[derive(Facet)]` value through phon's typed (schema-driven) path, mirroring
//! the slice of vox-postcard's surface the driver uses (`to_vec` / `from_slice`).
//! It derives the schema + descriptor from the facet `Shape`, lowers that to
//! phon IR, then runs the native JIT backend when this target supports it.
//!
//! The wire is **phon-compact** — fixed-width little-endian with `u32` length
//! prefixes and alignment padding — and is deliberately NOT byte-compatible with
//! the postcard wire it replaces. Swapping codecs breaks the old wire by design.
//!
//! Not yet here (follow-up): per-type descriptor/program caching.

use std::mem::MaybeUninit;

use facet::{Facet, PtrConst, Shape};
use phon::derive::{Derived, of, of_shape};
use phon_engine::{Registry, typed};
use phon_ir::Lowered;

pub mod schema;
pub use schema::{
    DecodeProgram, SchemaBundle, build_decode_program, decode_compat, decode_owned_with_program,
    decode_with_program, from_self_describing, parse_schema_bytes, recursive_schema_ids_for_shape,
    schema_bytes, schema_bytes_for_shape, schema_id_for_shape, to_self_describing,
};

/// Opaque-passthrough sentinel: build an `OpaqueSerialize` that emits already-encoded
/// `bytes` verbatim as the opaque inner content (no re-derive/re-encode). Used by the
/// `Payload` adapter to forward an already-encoded RPC payload (e.g. a proxied call).
pub use phon::derive::{RAW_OPAQUE_BYTES_SHAPE, RawOpaqueBytes, raw_opaque_bytes};

/// A codec error: the type could not be lowered to a phon schema, or the
/// value/bytes did not match it.
#[derive(Debug)]
pub struct Error(pub String);

impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for Error {}

fn lower_derived(type_name: &str, derived: &Derived) -> Result<Lowered, Error> {
    let reg = Registry::new(derived.schemas.clone());
    typed::lower_typed(&derived.descriptor, &derived.descriptor_blocks, &reg)
        .map_err(|e| Error(format!("lower {type_name}: {e:?}")))
}

unsafe fn encode_lowered(lowered: &Lowered, base: *const u8) -> Vec<u8> {
    #[cfg(all(target_os = "macos", target_arch = "aarch64"))]
    {
        let native = phon_jit::native::NativeEncode::compile_lowered(lowered);
        unsafe { native.run(base) }
    }

    #[cfg(not(all(target_os = "macos", target_arch = "aarch64")))]
    {
        unsafe { typed::encode_with(lowered, base) }
    }
}

unsafe fn decode_lowered(
    lowered: &Lowered,
    bytes: &[u8],
    base: *mut u8,
    type_name: &str,
) -> Result<(), Error> {
    #[cfg(all(target_os = "macos", target_arch = "aarch64"))]
    {
        let native = phon_jit::native::NativeDecode::compile_lowered(lowered);
        unsafe { native.run(bytes, base) }.map_err(|e| Error(format!("decode {type_name}: {e:?}")))
    }

    #[cfg(not(all(target_os = "macos", target_arch = "aarch64")))]
    {
        unsafe { typed::decode_with(lowered, bytes, base) }
            .map_err(|e| Error(format!("decode {type_name}: {e:?}")))
    }
}

/// Encode `value` to phon-compact bytes via its facet-derived schema.
///
/// # Errors
/// [`Error`] if `T` cannot be lowered to a phon schema or the value does not
/// match it.
// r[impl zerocopy.framing.value]
pub fn to_vec<'a, T: Facet<'a>>(value: &T) -> Result<Vec<u8>, Error> {
    let type_name = T::SHAPE.type_identifier;
    let derived = of::<T>().map_err(|e| Error(format!("derive {type_name}: {e}")))?;
    let lowered = lower_derived(type_name, &derived)?;
    // Safety: `value` is a live `T`; `lowered` was built from `T`'s descriptor.
    Ok(unsafe { encode_lowered(&lowered, (value as *const T).cast::<u8>()) })
}

/// Encode a type-erased value `(ptr, shape)` to phon-compact bytes via its
/// facet-derived schema — the shape-driven analog of [`to_vec`], used where the
/// concrete type isn't a generic param (e.g. the `Payload::Value` send path that
/// must pre-encode channel-bearing args out-of-band).
///
/// # Safety
/// `ptr` must point to an initialized value whose layout matches `shape`.
///
/// # Errors
/// [`Error`] if `shape` cannot be lowered to a phon schema or the value does not
/// match it.
// r[impl zerocopy.framing.value]
pub fn to_vec_for_shape(ptr: PtrConst, shape: &'static Shape) -> Result<Vec<u8>, Error> {
    let type_name = shape.type_identifier;
    let derived = of_shape(shape).map_err(|e| Error(format!("derive {type_name}: {e}")))?;
    let lowered = lower_derived(type_name, &derived)?;
    // Safety: `ptr` points to a live value of `shape`; `lowered` was built from
    // that shape's descriptor.
    Ok(unsafe { encode_lowered(&lowered, ptr.as_byte_ptr()) })
}

/// Decode `T` from phon-compact bytes, BORROWING from `bytes` (zero-copy): `&str`,
/// `&[u8]`, `Cow`, and opaque payloads point INTO `bytes`, so the decoded value may
/// not outlive it. The lifetime tie (`bytes: &'a [u8]`, `T: Facet<'a>`) enforces it.
///
/// This is the recv-path decode for the `Message` envelope: the payload field
/// decodes to a borrowed span and metadata strings borrow the backing.
///
/// # Errors
/// [`Error`] if `T` cannot be lowered, or the bytes are malformed for it.
// r[impl zerocopy.framing.value]
pub fn from_slice_borrowed<'a, T: Facet<'a>>(bytes: &'a [u8]) -> Result<T, Error> {
    let type_name = T::SHAPE.type_identifier;
    let derived = of::<T>().map_err(|e| Error(format!("derive {type_name}: {e}")))?;
    let lowered = lower_derived(type_name, &derived)?;
    let mut slot = MaybeUninit::<T>::uninit();
    // Safety: `lowered` was built from `T`'s descriptor; on `Ok`, decode has fully
    // initialized the slot. Borrowed fields point into `bytes`, which outlives the
    // returned `T` by the `'a` tie.
    unsafe {
        decode_lowered(&lowered, bytes, slot.as_mut_ptr().cast::<u8>(), type_name)?;
        Ok(slot.assume_init())
    }
}

/// Decode an owned `T` from phon-compact bytes via its facet-derived schema,
/// rejecting trailing bytes.
///
/// # Errors
/// [`Error`] if `T` cannot be lowered, or the bytes are malformed for it.
// r[impl zerocopy.framing.value]
pub fn from_slice<'a, T: Facet<'a>>(bytes: &[u8]) -> Result<T, Error> {
    let type_name = T::SHAPE.type_identifier;
    let derived = of::<T>().map_err(|e| Error(format!("derive {type_name}: {e}")))?;
    let lowered = lower_derived(type_name, &derived)?;
    let mut slot = MaybeUninit::<T>::uninit();
    // Safety: `lowered` was built from `T`'s descriptor; on `Ok`, decode has fully
    // initialized the slot.
    unsafe {
        decode_lowered(&lowered, bytes, slot.as_mut_ptr().cast::<u8>(), type_name)?;
        Ok(slot.assume_init())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use phon_engine::typed;
    use vox_types::{
        BindingDirection, ConnectionId, Message, MessagePayload, MethodId, Payload, RequestBody,
        RequestCall, RequestId, RequestMessage, SchemaBytes, SchemaMessage,
    };

    #[derive(Facet, Debug, PartialEq)]
    struct Point {
        x: u32,
        y: u32,
    }

    #[derive(Facet, Debug, PartialEq)]
    #[repr(u8)]
    enum Shape {
        Circle(f64),
        Rectangle { width: f64, height: f64 },
        Point,
    }

    #[derive(Facet, Debug, PartialEq)]
    struct Person {
        name: String,
        age: u32,
        email: Option<String>,
        tags: Vec<String>,
        home: Point,
        favorite: Shape,
        big: u64,
    }

    fn round_trip<T>(value: &T) -> T
    where
        T: Facet<'static> + std::fmt::Debug + PartialEq,
    {
        let bytes = to_vec(value).expect("encode");
        from_slice::<T>(&bytes).expect("decode")
    }

    #[test]
    fn round_trips_a_rich_struct() {
        let p = Person {
            name: "Ada".to_string(),
            age: 36,
            email: Some("ada@example.com".to_string()),
            tags: vec!["math".to_string(), "engine".to_string()],
            home: Point { x: 10, y: 20 },
            favorite: Shape::Rectangle {
                width: 3.0,
                height: 4.0,
            },
            big: 5_000_000_000,
        };
        assert_eq!(round_trip(&p), p);
    }

    #[test]
    fn round_trips_each_enum_variant() {
        assert_eq!(round_trip(&Shape::Circle(2.5)), Shape::Circle(2.5));
        assert_eq!(round_trip(&Shape::Point), Shape::Point);
        assert_eq!(
            round_trip(&Shape::Rectangle {
                width: 1.0,
                height: 2.0
            }),
            Shape::Rectangle {
                width: 1.0,
                height: 2.0
            },
        );
    }

    #[test]
    fn round_trips_empty_collections_and_none() {
        let p = Person {
            name: String::new(),
            age: 0,
            email: None,
            tags: Vec::new(),
            home: Point { x: 0, y: 0 },
            favorite: Shape::Point,
            big: 0,
        };
        assert_eq!(round_trip(&p), p);
    }

    fn interpreter_to_vec<'a, T: Facet<'a>>(value: &T) -> Vec<u8> {
        let type_name = T::SHAPE.type_identifier;
        let derived = of::<T>().expect("derive");
        let lowered = lower_derived(type_name, &derived).expect("lower");
        // Safety: `value` is a live `T`; `lowered` was built from `T`.
        unsafe { typed::encode_with(&lowered, (value as *const T).cast::<u8>()) }
    }

    #[test]
    fn native_message_schema_message_encode_matches_interpreter() {
        let message = Message {
            connection_id: ConnectionId(7),
            payload: MessagePayload::SchemaMessage(SchemaMessage {
                method_id: MethodId(0x0102_0304_0506_0708),
                direction: BindingDirection::Args,
                schemas: SchemaBytes(vec![0x18, 0x24, 0x42, 0x99]),
            }),
        };

        let native = to_vec(&message).expect("native encode");
        let interpreter = interpreter_to_vec(&message);
        assert_eq!(native, interpreter);
    }

    #[test]
    fn native_message_request_call_encode_matches_interpreter() {
        let args = [0x18, 0x24, 0x42, 0x99];
        let message = Message {
            connection_id: ConnectionId(7),
            payload: MessagePayload::RequestMessage(RequestMessage {
                id: RequestId(9),
                body: RequestBody::Call(RequestCall {
                    method_id: MethodId(0x0102_0304_0506_0708),
                    channels: Vec::new(),
                    metadata: Default::default(),
                    args: Payload::Encoded(&args),
                    schemas: SchemaBytes(Vec::new()),
                }),
            }),
        };

        let native = to_vec(&message).expect("native encode");
        let interpreter = interpreter_to_vec(&message);
        assert_eq!(native, interpreter);
    }
}
