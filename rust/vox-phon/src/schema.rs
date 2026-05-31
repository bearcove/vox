//! Schema exchange and **compatibility** decode through phon.
//!
//! A peer describes its types to the other side as phon **self-describing** schema
//! bytes (this is what CBOR used to carry). The receiver parses that closure into a
//! [`SchemaBundle`], then builds a compatibility decode program reconciling the
//! *writer's* schema against the *reader's* derived descriptor — phon's
//! `lower_decode` (`r[compat.plan-first]`). Every decode goes through this; there is
//! no same-version shortcut (the drift-free case is just the degenerate output of
//! the one program, `r[ir.inlining]`).
//!
//! Wire framing of a closure: `u64` root id, `u32` schema count, then each schema as
//! `u32` length + its [`schema_to_bytes`] self-describing bytes.

use std::mem::MaybeUninit;

use facet::{Facet, Shape};
use phon::derive::{of, of_shape};
use phon_engine::{Registry, typed};
use phon_ir::MemProgram;
use phon_schema::bytes::Reader;
use phon_schema::{Schema, SchemaId, schema_from_bytes, schema_to_bytes};

use crate::Error;

/// A decoded schema closure: the root type's id and every reachable composite
/// schema. The writer's view of a type, used to build a compat decode program.
#[derive(Clone, Debug)]
pub struct SchemaBundle {
    pub root: SchemaId,
    pub schemas: Vec<Schema>,
}

/// The phon schema closure of `T` (root id + every reachable composite schema),
/// encoded as self-describing bytes — what a peer sends so the receiver can build a
/// compatibility decode program for `T`.
///
/// # Errors
/// [`Error`] if `T` cannot be lowered to a phon schema.
pub fn schema_bytes<'a, T: Facet<'a>>() -> Result<Vec<u8>, Error> {
    let d = of::<T>().map_err(|e| Error(format!("derive {}: {e}", T::SHAPE.type_identifier)))?;
    Ok(encode_bundle(d.root, &d.schemas))
}

/// Like [`schema_bytes`] but from a reflected `Shape` directly (the send tracker
/// works with `&'static Shape`, not a generic `T`).
///
/// # Errors
/// [`Error`] if the shape cannot be lowered to a phon schema.
pub fn schema_bytes_for_shape(shape: &'static Shape) -> Result<Vec<u8>, Error> {
    let d = of_shape(shape).map_err(|e| Error(format!("derive {}: {e}", shape.type_identifier)))?;
    Ok(encode_bundle(d.root, &d.schemas))
}

/// Encode a `(root, schemas)` closure to self-describing bytes.
fn encode_bundle(root: SchemaId, schemas: &[Schema]) -> Vec<u8> {
    let mut out = Vec::new();
    out.extend_from_slice(&root.0.to_le_bytes());
    out.extend_from_slice(&(schemas.len() as u32).to_le_bytes());
    for s in schemas {
        let b = schema_to_bytes(s);
        out.extend_from_slice(&(b.len() as u32).to_le_bytes());
        out.extend_from_slice(&b);
    }
    out
}

/// Parse a schema closure produced by [`schema_bytes`].
///
/// # Errors
/// [`Error`] for malformed or truncated input.
pub fn parse_schema_bytes(bytes: &[u8]) -> Result<SchemaBundle, Error> {
    let mut r = Reader::new(bytes);
    let root = SchemaId(
        r.read_u64()
            .map_err(|e| Error(format!("schema bundle root: {e:?}")))?,
    );
    let count = r
        .read_u32()
        .map_err(|e| Error(format!("schema bundle count: {e:?}")))? as usize;
    let mut schemas = Vec::with_capacity(count.min(1024));
    for _ in 0..count {
        let len = r
            .read_u32()
            .map_err(|e| Error(format!("schema bundle entry length: {e:?}")))?
            as usize;
        let slice = r
            .read_slice(len)
            .map_err(|e| Error(format!("schema bundle entry body: {e:?}")))?;
        schemas.push(schema_from_bytes(slice).map_err(|e| Error(format!("schema decode: {e:?}")))?);
    }
    if r.remaining() != 0 {
        return Err(Error(format!(
            "schema bundle has {} trailing bytes",
            r.remaining()
        )));
    }
    Ok(SchemaBundle { root, schemas })
}

/// A prebuilt compatibility decode program: the writer schema reconciled against the
/// reader type `T`'s descriptor, lowered once. Build it per `(writer root, T)` and
/// reuse it for every message — the reconciliation cost is paid here, not per decode.
#[derive(Clone)]
pub struct DecodeProgram(MemProgram);

// A built program is immutable, and its thunk `ctx` pointers are all `&'static`
// references (facet defs / adapter defs) cast to `*const ()` — morally `Send + Sync`,
// but the raw-pointer representation loses the auto-trait. Re-assert it so a program
// can be cached on the shared `SchemaRecvTracker` and run from any thread.
// Safety: immutable after build; thunk pointers are `&'static` / stateless.
unsafe impl Send for DecodeProgram {}
unsafe impl Sync for DecodeProgram {}

/// Build the compat decode program reconciling `writer`'s schema against `T`'s
/// derived descriptor (`r[compat.plan-first]`). Fails if the schemas are
/// incompatible — before any bytes are touched.
///
/// # Errors
/// [`Error`] if `T` cannot be derived, the writer root is unknown, or the schemas
/// cannot be reconciled.
pub fn build_decode_program<'a, T: Facet<'a>>(
    writer: &SchemaBundle,
) -> Result<DecodeProgram, Error> {
    let reader =
        of::<T>().map_err(|e| Error(format!("derive {}: {e}", T::SHAPE.type_identifier)))?;
    // The registry must resolve both the writer's refs and the reader's refs.
    let mut schemas = writer.schemas.clone();
    for s in &reader.schemas {
        if !schemas.iter().any(|x| x.id == s.id) {
            schemas.push(s.clone());
        }
    }
    let reg = Registry::new(schemas);
    let program = typed::lower_decode(writer.root, &reader.descriptor, &reg)
        .map_err(|e| Error(format!("lower_decode {}: {e:?}", T::SHAPE.type_identifier)))?;
    Ok(DecodeProgram(program))
}

/// Decode `bytes` into `T` through a prebuilt compat [`DecodeProgram`], BORROWING
/// from `bytes` (zero-copy). The program and `T` must match.
///
/// # Errors
/// [`Error`] for malformed or trailing input.
pub fn decode_with_program<'a, T: Facet<'a>>(
    program: &DecodeProgram,
    bytes: &'a [u8],
) -> Result<T, Error> {
    let mut slot = MaybeUninit::<T>::uninit();
    // Safety: `program` was lowered for `T`'s descriptor; on `Ok`, `decode_with`
    // fully initializes the slot. Borrowed fields point into `bytes` (the `'a` tie).
    unsafe {
        typed::decode_with(&program.0, bytes, slot.as_mut_ptr().cast::<u8>())
            .map_err(|e| Error(format!("decode {}: {e:?}", T::SHAPE.type_identifier)))?;
        Ok(slot.assume_init())
    }
}

/// Decode an OWNED `T` (`T: Facet<'static>`) through a prebuilt compat
/// [`DecodeProgram`], independent of the input's lifetime. An owned wire type's
/// descriptor uses owned vtables (allocating `String`/`Vec`, never `&str`/`Cow`), so
/// the result borrows nothing from `bytes` and `bytes` may be short-lived.
///
/// # Errors
/// [`Error`] for malformed or trailing input.
pub fn decode_owned_with_program<T: Facet<'static>>(
    program: &DecodeProgram,
    bytes: &[u8],
) -> Result<T, Error> {
    let mut slot = MaybeUninit::<T>::uninit();
    // Safety: `program` was lowered for `T`'s descriptor; `T: Facet<'static>` means
    // the descriptor is fully owned (no borrowed leaves), so the decoded value owns
    // its data and does not reference `bytes`.
    unsafe {
        typed::decode_with(&program.0, bytes, slot.as_mut_ptr().cast::<u8>())
            .map_err(|e| Error(format!("decode {}: {e:?}", T::SHAPE.type_identifier)))?;
        Ok(slot.assume_init())
    }
}

/// Convenience: build a one-shot compat program and decode in one step. Prefer
/// caching a [`DecodeProgram`] across messages where the writer schema is stable.
///
/// # Errors
/// As [`build_decode_program`] and [`decode_with_program`].
pub fn decode_compat<'a, T: Facet<'a>>(bytes: &'a [u8], writer: &SchemaBundle) -> Result<T, Error> {
    let program = build_decode_program::<T>(writer)?;
    decode_with_program::<T>(&program, bytes)
}

/// Encode `value` as a SELF-CONTAINED message: its phon schema closure (`u32` length
/// then [`schema_bytes`]) followed by its compact value. Used where no schema was
/// pre-exchanged — the handshake — so the message carries the schema needed to decode
/// it (the phon analog of a CBOR-style self-describing typed value).
///
/// # Errors
/// [`Error`] if `T` cannot be derived or encoded.
pub fn to_self_describing<'a, T: Facet<'a>>(value: &T) -> Result<Vec<u8>, Error> {
    let schema = schema_bytes::<T>()?;
    let value_bytes = crate::to_vec(value)?;
    let mut out = Vec::with_capacity(4 + schema.len() + value_bytes.len());
    out.extend_from_slice(&(schema.len() as u32).to_le_bytes());
    out.extend_from_slice(&schema);
    out.extend_from_slice(&value_bytes);
    Ok(out)
}

/// Decode a self-contained message produced by [`to_self_describing`] into an OWNED
/// `T`: parse the embedded writer schema closure, reconcile it against `T`
/// (`r[compat.plan-first]`), and decode the value. The handshake decode — so even the
/// bootstrap message reconciles writer↔reader rather than assuming same-version.
///
/// # Errors
/// [`Error`] for malformed framing, an undecodable schema, or incompatible schemas.
pub fn from_self_describing<T: Facet<'static>>(bytes: &[u8]) -> Result<T, Error> {
    let mut r = Reader::new(bytes);
    let schema_len =
        r.read_u32()
            .map_err(|e| Error(format!("self-describing schema length: {e:?}")))? as usize;
    let schema = r
        .read_slice(schema_len)
        .map_err(|e| Error(format!("self-describing schema body: {e:?}")))?;
    let value = &bytes[4 + schema_len..];
    let writer = parse_schema_bytes(schema)?;
    let program = build_decode_program::<T>(&writer)?;
    decode_owned_with_program::<T>(&program, value)
}

#[cfg(test)]
mod tests {
    use super::*;

    // Writer and reader drift: the writer struct has an extra field the reader
    // lacks (skipped), and the reader has a defaulted field the writer lacks
    // (defaulted). The decode reconciles both — the compat path, exercised end to end
    // over a real schema exchange.
    #[derive(Facet)]
    struct Writer {
        a: u32,
        gone: String,
        b: u32,
    }

    #[derive(Facet, Debug, PartialEq)]
    struct Reader2 {
        a: u32,
        b: u32,
        #[facet(default)]
        added: u32,
    }

    #[test]
    fn compat_decode_reconciles_writer_and_reader_drift() {
        // The writer sends its schema closure.
        let writer_bytes = schema_bytes::<Writer>().expect("writer schema bytes");
        let bundle = parse_schema_bytes(&writer_bytes).expect("parse bundle");

        // The writer encodes a value with ITS schema.
        let value = Writer {
            a: 11,
            gone: "discard".to_string(),
            b: 22,
        };
        let wire = crate::to_vec(&value).expect("encode writer value");

        // The reader reconciles the writer schema against its own type and decodes.
        let decoded: Reader2 = decode_compat(&wire, &bundle).expect("compat decode");
        assert_eq!(
            decoded,
            Reader2 {
                a: 11,
                b: 22,
                added: 0
            }
        );
    }

    #[test]
    fn schema_bundle_round_trips() {
        let bytes = schema_bytes::<Writer>().expect("schema bytes");
        let bundle = parse_schema_bytes(&bytes).expect("parse");
        let d = of::<Writer>().expect("derive");
        assert_eq!(bundle.root, d.root);
        assert_eq!(bundle.schemas.len(), d.schemas.len());
    }
}
