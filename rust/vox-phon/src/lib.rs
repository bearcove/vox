//! phon as a vox codec.
//!
//! This is the data-plane adapter for the codec migration: encode/decode a
//! `#[derive(Facet)]` value through phon's typed (schema-driven) path, mirroring
//! the slice of vox-postcard's surface the driver uses (`to_vec` / `from_slice`).
//! It derives the schema + descriptor from the facet `Shape`, then runs phon's
//! interpreter encode/decode.
//!
//! The wire is **phon-compact** — fixed-width little-endian with `u32` length
//! prefixes and alignment padding — and is deliberately NOT byte-compatible with
//! the postcard wire it replaces. Swapping codecs breaks the old wire by design.
//!
//! Not yet here (follow-ups): per-type descriptor/program caching, the native
//! (copy-and-patch) JIT fast path, borrowed/zero-copy decode, and the
//! `Message`-envelope handling for its opaque `Payload`/`CborPayload` fields.

use std::mem::MaybeUninit;

use facet::Facet;
use phon::derive::of;
use phon_engine::{Registry, typed};

pub mod schema;
pub use schema::{
    DecodeProgram, SchemaBundle, build_decode_program, decode_compat, decode_with_program,
    parse_schema_bytes, schema_bytes, schema_bytes_for_shape,
};

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

/// Encode `value` to phon-compact bytes via its facet-derived schema.
///
/// # Errors
/// [`Error`] if `T` cannot be lowered to a phon schema or the value does not
/// match it.
pub fn to_vec<'a, T: Facet<'a>>(value: &T) -> Result<Vec<u8>, Error> {
    let derived =
        of::<T>().map_err(|e| Error(format!("derive {}: {e}", T::SHAPE.type_identifier)))?;
    let reg = Registry::new(derived.schemas);
    // Safety: `value` is a live `T`; `derived.descriptor` describes `T`'s layout.
    unsafe { typed::encode((value as *const T).cast::<u8>(), &derived.descriptor, &reg) }
        .map_err(|e| Error(format!("encode {}: {e:?}", T::SHAPE.type_identifier)))
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
pub fn from_slice_borrowed<'a, T: Facet<'a>>(bytes: &'a [u8]) -> Result<T, Error> {
    let derived =
        of::<T>().map_err(|e| Error(format!("derive {}: {e}", T::SHAPE.type_identifier)))?;
    let reg = Registry::new(derived.schemas);
    let mut slot = MaybeUninit::<T>::uninit();
    // Safety: `derived.descriptor` describes `T`; on `Ok`, `decode` has fully
    // initialized the slot. Borrowed fields point into `bytes`, which outlives the
    // returned `T` by the `'a` tie.
    unsafe {
        typed::decode(
            bytes,
            &derived.descriptor,
            &reg,
            slot.as_mut_ptr().cast::<u8>(),
        )
        .map_err(|e| Error(format!("decode {}: {e:?}", T::SHAPE.type_identifier)))?;
        Ok(slot.assume_init())
    }
}

/// Decode an owned `T` from phon-compact bytes via its facet-derived schema,
/// rejecting trailing bytes.
///
/// # Errors
/// [`Error`] if `T` cannot be lowered, or the bytes are malformed for it.
pub fn from_slice<'a, T: Facet<'a>>(bytes: &[u8]) -> Result<T, Error> {
    let derived =
        of::<T>().map_err(|e| Error(format!("derive {}: {e}", T::SHAPE.type_identifier)))?;
    let reg = Registry::new(derived.schemas);
    let mut slot = MaybeUninit::<T>::uninit();
    // Safety: `derived.descriptor` describes `T`; on `Ok`, `decode` has fully
    // initialized the slot.
    unsafe {
        typed::decode(
            bytes,
            &derived.descriptor,
            &reg,
            slot.as_mut_ptr().cast::<u8>(),
        )
        .map_err(|e| Error(format!("decode {}: {e:?}", T::SHAPE.type_identifier)))?;
        Ok(slot.assume_init())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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
}
