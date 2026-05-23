+++
title = "Telex"
description = "Binary value format, framing modes, and schema-evolution constraints"
weight = 13
+++

Telex is a binary value format for Facet-shaped values. Vox uses it for
protocol messages, handshake values, schema payloads, and application payloads.
It has two framing modes:

- **Self-describing mode** carries structural tags inline and can be decoded
  without a schema. It is used for the session handshake and schema payloads.
- **Compact mode** omits structural tags and is decoded against schemas already
  known to both peers. It is used for steady-state RPC messages and payloads.

The two modes share the same scalar and byte-level leaf encoding. The only
difference is whether the structural tag stream is present in the bytes.
The name is deliberately not Vox-branded: Telex is the message format beneath
Vox, and can be specified independently from the Vox RPC protocol.

# Format contract

> r[telex.format]
>
> Vox implementations MUST use Telex as the value format for protocol
> messages, handshake values, schema payloads, and application payloads. A
> conforming implementation MUST NOT require a second value codec to decode one
> logical Vox message.

> r[telex.modes]
>
> Telex defines exactly two framing modes:
>
> - **Self-describing**: every value carries enough structural tags to decode
>   into a generic value tree without prior schema agreement.
> - **Compact**: values are encoded against a known schema, so field, variant,
>   and aggregate structure are taken from that schema instead of repeated in
>   the byte stream.
>
> The mode is a framing choice around one shared value codec, not a choice
> between unrelated formats.

> r[telex.mode.self-describing]
>
> Self-describing mode MUST be decodable with zero application schema knowledge.
> A decoder only needs the fixed Telex tag vocabulary and scalar encodings
> in this chapter. It MUST be able to materialize a generic value tree that can
> later be deserialized tolerantly into local protocol types.

> r[telex.mode.compact]
>
> Compact mode MUST be decoded only with an agreed local schema, a remote
> schema, and the translation plan between them. Compact bytes MUST NOT contain
> field names, type names, or structural tags except for the scalar bytes and
> compact aggregate framing defined here.

> r[telex.one-codec]
>
> Self-describing and compact mode MUST share one implementation model: one
> scalar codec, one aggregate framing model, one error type, one translation IR,
> and one portable interpreter. Native JIT codegen is an accelerator for that
> IR, not a separate wire semantics.

# Byte order and lengths

> r[telex.endianness]
>
> All fixed-width numeric values in Telex MUST be little-endian.

> r[telex.no-varint]
>
> Telex MUST NOT use varints for integers, lengths, enum discriminants, or
> type identifiers. Every integer has the width assigned by its type or by this
> chapter.

> r[telex.length.u32]
>
> Variable-length byte regions and aggregate counts MUST use a fixed-width
> little-endian `u32` length unless a requirement in this chapter explicitly
> assigns a different width. Length fields count bytes for byte regions and
> elements for element-counted aggregates.

> r[telex.length.canonical-width]
>
> Length and count widths are part of the canonical Telex byte format. An
> encoder MUST NOT choose a narrower width for small values, schemas, local
> transports, or uncompressed links. Compression, when used, happens below
> Telex value framing and MUST NOT change the uncompressed Telex bytes.

> r[telex.length.bounds]
>
> A decoder MUST reject a value whose length or count would read past the input,
> exceed the implementation's configured message limit, or overflow host
> pointer arithmetic.

# Scalar encoding

> r[telex.scalar.bool]
>
> `bool` is encoded as one byte: `0x00` for `false`, `0x01` for `true`.
> Any other byte is invalid.

> r[telex.scalar.unsigned]
>
> Unsigned integers are encoded at their declared width: `u8`, `u16`, `u32`,
> `u64`, and `u128`.

> r[telex.scalar.signed]
>
> Signed integers are encoded at their declared width in two's-complement
> little-endian form: `i8`, `i16`, `i32`, `i64`, and `i128`.

> r[telex.scalar.float]
>
> `f32` and `f64` are encoded as their IEEE 754 bit pattern in little-endian
> order. NaN payload bits are preserved.

> r[telex.scalar.char]
>
> `char` is encoded as a little-endian `u32` Unicode scalar value. A decoder
> MUST reject surrogate code points and values greater than `0x10FFFF`.

> r[telex.scalar.string]
>
> Strings are encoded as `[byte_len: u32 LE][utf8 bytes]`. A decoder MUST
> reject invalid UTF-8.

> r[telex.scalar.bytes]
>
> Byte sequences are encoded as `[byte_len: u32 LE][raw bytes]`.

> r[telex.scalar.unit]
>
> Unit encodes to zero bytes in compact mode. In self-describing mode, unit is
> represented by the unit tag with no payload.

> r[telex.scalar.never]
>
> The never type has no value. Encoders MUST NOT emit a never value. A decoder
> asked to materialize a never value MUST fail.

# Self-describing tags

Self-describing mode prefixes each value with one tag byte from this table. The
assigned bytes are the permanent bootstrap contract for Telex.

> r[telex.tags]
>
> Self-describing mode MUST use the following tag byte assignments:
>
> | Tag | Telex kind | Facet source |
> |-----|------------|--------------|
> | `0x00` | unit | `ScalarType::Unit`; unit struct/variant payloads |
> | `0x01` | bool | `ScalarType::Bool` |
> | `0x02` | u8 | `ScalarType::U8` |
> | `0x03` | u16 | `ScalarType::U16` |
> | `0x04` | u32 | `ScalarType::U32` |
> | `0x05` | u64 | `ScalarType::U64` |
> | `0x06` | u128 | `ScalarType::U128` |
> | `0x07` | i8 | `ScalarType::I8` |
> | `0x08` | i16 | `ScalarType::I16` |
> | `0x09` | i32 | `ScalarType::I32` |
> | `0x0A` | i64 | `ScalarType::I64` |
> | `0x0B` | i128 | `ScalarType::I128` |
> | `0x0C` | f32 | `ScalarType::F32` |
> | `0x0D` | f64 | `ScalarType::F64` |
> | `0x0E` | char | `ScalarType::Char` |
> | `0x0F` | string | `ScalarType::Str`, `ScalarType::String`, `ScalarType::CowStr` |
> | `0x10` | bytes | byte slices, byte vectors, and schema `bytes` primitives |
> | `0x11` | payload | schema opaque payload primitive |
> | `0x12` | list | `Def::List`, `Def::Slice`, and `Type::Sequence(SequenceType::Slice)` |
> | `0x13` | set | `Def::Set` |
> | `0x14` | map | `Def::Map` |
> | `0x15` | array | `Def::Array` and `Type::Sequence(SequenceType::Array)` |
> | `0x16` | n-dimensional array | `Def::NdArray` |
> | `0x17` | tuple | `StructKind::Tuple`, `StructKind::TupleStruct`, and schema tuples |
> | `0x18` | struct | `Type::User(UserType::Struct(_))` with named fields |
> | `0x19` | enum variant | `Type::User(UserType::Enum(_))` and `Def::Result` |
> | `0x1A` | option none | `Def::Option` |
> | `0x1B` | option some | `Def::Option` |
> | `0x1C` | dynamic value | `Def::DynamicValue` |
> | `0x80..0xFF` | extension | future Telex extension tags |
>
> Tags `0x1D..0x7F` are reserved and MUST NOT be emitted. Tags
> `0x80..0xFF` MUST follow the extension envelope defined by
> `r[telex.tags.extension]`.

> r[telex.tags.scalar-payload]
>
> For scalar tags, the tag byte is followed by the same scalar payload bytes
> used in compact mode.

> r[telex.tags.extension]
>
> Extension tags (`0x80..0xFF`) MUST be followed by:
>
> ```text
> extension_id: u32 LE
> payload_len: u32 LE
> payload_bytes: [u8; payload_len]
> ```
>
> A decoder that does not understand an extension tag or extension ID MUST
> preserve the payload as an opaque extension value in the generic value tree.
> It MUST NOT silently drop the payload.

> r[telex.tags.forward-contract]
>
> The non-extension tag vocabulary is the self-describing bootstrap contract.
> The meaning of tags `0x00..0x1C` MUST NOT change. Evolving protocol structs,
> schema structs, or application structs MUST NOT require adding a tag byte.
> A future Facet scalar or shape that cannot be represented through existing
> tags MAY use an extension tag, but old decoders will materialize it only as
> an opaque extension value.

> r[telex.tags.unsupported-facet]
>
> Facet scalar and shape variants not listed in `r[telex.tags]` do not have an
> implicit Telex representation. In particular, `usize`, `isize`, network
> address scalars, `ConstTypeId`, raw pointers, function pointers, unions, and
> undefined shapes MUST either be mapped by an explicit schema-defined type or
> rejected before encoding.

# Aggregate encoding

> r[telex.aggregate.option]
>
> In compact mode, `Option<T>` is encoded as one byte followed by an optional
> payload: `0x00` for none, `0x01` followed by the compact `T` encoding for
> some. Any other tag is invalid. In self-describing mode, `option none` has no
> payload and `option some` is followed by one self-described value.

> r[telex.aggregate.list]
>
> Lists and slices are encoded as `[count: u32 LE][element bytes...]`, with
> elements encoded in order.

> r[telex.aggregate.set]
>
> Sets are encoded as `[count: u32 LE][element bytes...]`. The element order
> MUST be deterministic for a given set value and schema. A receiver MUST NOT
> depend on set element order for semantics.

> r[telex.aggregate.map]
>
> Maps are encoded as `[count: u32 LE][key value pairs...]`. Keys are encoded
> with the same value codec as any other value; map keys are not restricted to
> strings.

> r[telex.aggregate.array]
>
> Fixed-size arrays encode only their element bytes in compact mode; the length
> comes from the schema. In self-describing mode, an array carries
> `[count: u32 LE]` before the elements.

> r[telex.aggregate.nd-array]
>
> N-dimensional arrays carry a shape header followed by contiguous element
> bytes: `[rank: u32 LE][dim_0: u64 LE]...[dim_n: u64 LE][element bytes...]`.
> The product of the dimensions is the element count and MUST be checked for
> overflow.

> r[telex.aggregate.tuple]
>
> Tuples encode their elements in tuple order. Compact tuple arity comes from
> the schema. Self-describing tuples carry `[count: u32 LE]` before the
> elements.

> r[telex.aggregate.struct.self-describing]
>
> A self-describing struct is encoded as:
>
> ```text
> tag(struct)
> field_count: u32 LE
> repeated field_count times:
>   field_name: string payload without an extra string tag
>   field_len: u32 LE
>   field_value: self-described value bytes
> ```
>
> Field names are part of the self-describing stream so the value can be
> tolerantly deserialized into an evolved local struct.

> r[telex.aggregate.struct.compact]
>
> A compact struct is encoded in the sender's declaration order. Every field is
> encoded as:
>
> ```text
> field_len: u32 LE
> field_value: compact value bytes
> ```
>
> `field_len` counts only `field_value`, not the length prefix. This prefix is
> required for every struct field, even when the sender believes all receivers
> know the field.

> r[telex.aggregate.field-prefix]
>
> Compact struct fields and compact enum tuple/struct payload fields MUST
> always carry the `u32` field-length prefix. Encoders MUST NOT omit it when
> local and remote schemas are identical, when a field is fixed-width, or when
> the selected link is local or uncompressed.

> r[telex.aggregate.enum.compact]
>
> A compact enum is encoded as `[variant_index: u32 LE][payload bytes]`, where
> `variant_index` is the sender schema's declaration index. Unit variants have
> no payload. Newtype, tuple, and struct variant payload fields use the same
> per-field `u32` length prefix required for struct fields.

> r[telex.aggregate.enum.self-describing]
>
> A self-describing enum variant is encoded as:
>
> ```text
> tag(enum variant)
> variant_name: string payload without an extra string tag
> payload_len: u32 LE
> payload_value: self-described value bytes
> ```
>
> Unit variants use a unit payload value. Struct variants use a
> self-describing struct payload value. Tuple variants use a self-describing
> tuple payload value.

> r[telex.aggregate.dynamic-value]
>
> A dynamic value is encoded as one nested self-described value. This is true in
> both modes: compact mode reaches the dynamic-value field from schema, then
> the field payload switches to a nested self-describing value.

# Skipping and schema evolution

> r[telex.skip.struct-field]
>
> A decoder skipping an unknown compact struct field MUST skip it by reading
> its `u32` field length and advancing exactly that many bytes. It MUST NOT
> recursively interpret the field to discover its end.

> r[telex.skip.enum-field]
>
> A decoder skipping an unknown field inside an enum variant payload MUST use
> the same `u32` field-length rule as struct fields.

> r[telex.translation.name-matching]
>
> Struct fields and enum variants are matched by name using the exchanged
> schema. Compact bytes remain positional; the translation plan maps remote
> positions to local positions before decode code is built.

> r[telex.translation.default-fill]
>
> A local field missing from the remote schema MUST be filled by the decoder
> built for that remote/local schema pair if the local field has a default.
> Missing local fields without defaults are a translation-plan error.

> r[telex.translation.compiled]
>
> On native targets, the translation plan MUST be consumed when the decoder is
> built. The hot receive path MUST NOT interpret a translation plan per
> message.

> r[telex.translation.no-evolution-fallback]
>
> Skip, default-fill, and reorder are normal schema-evolution operations. A
> native decoder MUST be able to compile all three for compact Telex
> values. Encountering an evolved but compatible schema MUST NOT force the
> entire message onto an interpreter path.

# Bootstrap and schema payloads

> r[telex.bootstrap.self-describing]
>
> Session handshake messages MUST be encoded in self-describing mode. The
> handshake MUST NOT depend on a compiled-in schema version shared by both
> peers.

> r[telex.schema.self-describing]
>
> Schema payloads MUST be encoded in self-describing mode. A receiver first
> decodes the bytes into a generic value tree, then tolerantly deserializes
> that value into its local schema data model.

> r[telex.value.generic]
>
> A self-describing decoder MUST be able to materialize a generic value tree
> containing all known Telex primitives and aggregates, plus opaque extension
> values for unknown extension tags. The generic value tree is the only input
> required to materialize handshake and schema values into local types.

> r[telex.schema.meta-evolution]
>
> The schema data model itself is allowed to evolve. Extra fields in a remote
> schema value MUST be ignored when deserializing into an older local schema
> model, and missing fields MUST be filled from local defaults when available.

> r[telex.schema.tolerant-materialization]
>
> Materializing a generic schema value into the local schema data model MUST
> match struct fields and enum variants by name. Extra remote fields MUST be
> ignored, missing local fields MUST use defaults when available, and missing
> required fields or incompatible field shapes MUST be reported before compact
> payload translation begins.

> r[telex.schema.unknown-type]
>
> If a self-described schema value contains a type shape that the local schema
> model cannot represent, the receiver MUST preserve it as a generic value for
> diagnostics and MUST fail translation-plan construction for payloads that
> require that shape.

# Compression

> r[telex.compression]
>
> Wire compression is negotiated for a link attachment below Vox value framing.
> Compression, when enabled, wraps the byte stream carrying link payloads; it
> MUST NOT change individual value encodings or schema hashes.

> r[telex.compression.modes]
>
> The baseline compression mode is `none`. Any compressed mode MUST name its
> algorithm explicitly in a dedicated transport-prologue compression field
> before compressed bytes are sent. Compression MUST NOT be encoded by
> overloading the conduit or transport mode field.

> r[telex.compression.streaming]
>
> Compression for stream-like remote links SHOULD be streaming over the link
> byte stream, not a fresh compressor per Vox message. Per-message compression
> loses the recurring schema and field-layout context that makes fixed-width
> Telex bytes compress well.

> r[telex.compression.stream-state]
>
> A streaming compressed link has compression state scoped to the link
> attachment. A message captured from the middle of that stream is not
> independently decompressible unless the capture also includes all preceding
> compressed bytes or an explicit compression checkpoint. Tools that require
> independently decodable messages MUST disable compression or record such
> checkpoints.

# Decoder conformance

> r[telex.decoder.equivalence]
>
> All conforming Telex decoders for the same target type, remote schema, local
> schema, and translation plan MUST produce the same decoded value or the same
> error classification, regardless of whether they are reflective, interpreted,
> JIT-compiled, or otherwise optimized.

> r[telex.decoder.compat-ops]
>
> A conforming compact decoder MUST implement the schema-evolution operations
> required by this chapter, including skip, default-fill, and reorder. These
> operations are part of normal Telex semantics, not optional slow paths.

> r[telex.oracle.reflective]
>
> A reflective decoder that walks facet shapes directly MAY exist as a test and
> CI oracle. It MUST NOT be required as a shipped runtime path for compact
> Telex decoding.
