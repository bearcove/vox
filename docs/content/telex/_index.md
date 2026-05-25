+++
title = "telex"
description = "Binary serialization format for typed values"
weight = 13
+++

telex is a language-independent binary serialization format for typed values.
It defines a small value model, a self-describing byte form, and a compact byte
form for values whose type is known out of band.

telex does not depend on any programming language, reflection system, RPC
protocol, or transport. Those systems may map their own type information onto
the telex value model, but that mapping is outside the core byte format.

# Value model

> r[telex.value-model]
>
> A telex value is one of the following kinds:
>
> | Kind | Shape |
> |------|-------|
> | unit | empty value |
> | bool | true or false |
> | unsigned integer | `u8`, `u16`, `u32`, `u64`, or `u128` |
> | signed integer | `i8`, `i16`, `i32`, `i64`, or `i128` |
> | float | `f32` or `f64` IEEE 754 value |
> | char | Unicode scalar value |
> | string | UTF-8 byte sequence |
> | bytes | arbitrary byte sequence |
> | payload | opaque byte payload |
> | list | ordered homogeneous sequence |
> | set | unordered homogeneous collection with unique elements |
> | map | key-value collection with unique keys |
> | array | fixed-shape homogeneous array with one or more dimensions |
> | tuple | ordered fixed-arity product |
> | struct | named-field product |
> | enum variant | named alternative with payload |
> | option | none or some value |
> | dynamic value | nested self-described value |
> | extension | extension-defined payload |

> r[telex.value-model.extension-form]
>
> `extension` is a self-describing-only value kind. Compact schemas MUST NOT
> name `extension` as a compact kind, and compact values MUST NOT contain
> extension tags in schema-driven positions.
>
> A compact dynamic-value field may still contain an extension value, because
> `r[telex.aggregate.dynamic-value]` explicitly embeds a nested self-described
> value.

> r[telex.value-kind.preserved]
>
> The telex value kind is part of the value, not an implementation detail of
> the body bytes. A decoded generic telex value includes one kind from
> `r[telex.value-model]`.
>
> Self-describing form carries the kind as the leading tag byte. For
> compact-capable kinds, compact form omits that tag byte, but the external
> schema supplies the same kind the tag would have supplied. Body grammars may
> reuse the same byte skeleton without merging the corresponding value kinds;
> for example, `list`, `set`, and rank-1 `array` are distinct telex kinds even
> though each contains a sequence of element bytes.

# Encoding forms

> r[telex.forms]
>
> telex defines two byte forms for values:
>
> - **Self-describing form**: every value begins with a tag byte from
>   `r[telex.tags]` that identifies the value kind. Aggregate bodies contain
>   nested self-described values.
> - **Compact form**: the value kind and aggregate structure are supplied by a
>   schema outside the byte stream. Compact bytes contain scalar payloads,
>   aggregate counts, and aggregate payloads, but not tag bytes, field names,
>   variant names, or type names.

> r[telex.mode.self-describing]
>
> A self-describing value is decoded using only the fixed tag vocabulary and
> payload rules in this document. The leading tag selects the telex value kind
> and the body grammar for the bytes that follow. The value can be materialized
> as a generic telex value without any external schema.

> r[telex.mode.compact]
>
> A compact value is decoded using an external schema that supplies its kind and
> aggregate structure. Scalar payloads and aggregate counts have the same byte
> representation in compact form as they do inside self-describing form. The
> schema, not the body skeleton alone, determines
> whether those bytes are a list, set, array, tuple, struct, or other telex kind.

# Self-describing tags

Self-describing mode prefixes each value with one tag byte from this table. The
assigned bytes are the permanent bootstrap contract for telex.

> r[telex.tags]
>
> Self-describing mode MUST use the following tag byte assignments:
>
> | Tag | Kind | Body |
> |-----|------|------|
> | `0x00` | unit | empty |
> | `0x01` | bool | one byte, `0x00` or `0x01` |
> | `0x02` | u8 | 1 little-endian byte |
> | `0x03` | u16 | 2 little-endian bytes |
> | `0x04` | u32 | 4 little-endian bytes |
> | `0x05` | u64 | 8 little-endian bytes |
> | `0x06` | u128 | 16 little-endian bytes |
> | `0x07` | i8 | 1 little-endian byte |
> | `0x08` | i16 | 2 little-endian bytes |
> | `0x09` | i32 | 4 little-endian bytes |
> | `0x0A` | i64 | 8 little-endian bytes |
> | `0x0B` | i128 | 16 little-endian bytes |
> | `0x0C` | f32 | 4 little-endian IEEE 754 bytes |
> | `0x0D` | f64 | 8 little-endian IEEE 754 bytes |
> | `0x0E` | char | `u32` Unicode scalar value |
> | `0x0F` | string | `byte_len: u32 LE` followed by UTF-8 bytes |
> | `0x10` | bytes | `byte_len: u32 LE` followed by raw bytes |
> | `0x11` | payload | `byte_len: u32 LE` followed by opaque bytes |
> | `0x12` | list | `r[telex.aggregate.list]` |
> | `0x13` | set | `r[telex.aggregate.set]` |
> | `0x14` | map | `r[telex.aggregate.map]` |
> | `0x15` | array | `r[telex.aggregate.array]` |
> | `0x16` | tuple | `r[telex.aggregate.tuple]` |
> | `0x17` | struct | `r[telex.aggregate.struct.self-describing]` |
> | `0x18` | enum variant | `r[telex.aggregate.enum.self-describing]` |
> | `0x19` | option none | empty |
> | `0x1A` | option some | one self-described value |
> | `0x1B` | dynamic value | `r[telex.aggregate.dynamic-value]` |
> | `0x80..0xFF` | extension | `r[telex.tags.extension]` |
>
> Tags `0x1C..0x7F` are reserved; encodings containing them are invalid. Tags
> `0x80..0xFF` use the extension envelope defined by `r[telex.tags.extension]`.

> r[telex.tags.scalar-payload]
>
> For scalar tags (`0x00` through `0x11`), the tag byte is followed by the
> same scalar payload bytes used in compact mode.

> r[telex.tags.aggregate-payload]
>
> For aggregate tags (list `0x12`, set `0x13`, map `0x14`, array `0x15`,
> tuple `0x16`, struct `0x17`, enum variant `0x18`, option some `0x1A`,
> dynamic value `0x1B`), the tag byte is followed
> by the body defined in `r[telex.aggregate.*]` for that kind. Within a
> self-describing aggregate body, every element, key, value, and field-value
> is itself a self-described telex value beginning with its own tag byte. The
> sole exceptions are field and variant *names* in
> `r[telex.aggregate.struct.self-describing]` and
> `r[telex.aggregate.enum.self-describing]`, which are emitted as raw
> length-prefixed UTF-8 without the string tag.

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
> A decoder that does not understand an extension tag or extension ID preserves
> the payload as an opaque extension value in the generic telex value.

> r[telex.tags.forward-contract]
>
> The non-extension tag vocabulary is the self-describing bootstrap contract.
> Tags `0x00..0x1B` have the meanings assigned in `r[telex.tags]`. A future
> value kind that cannot be represented through existing tags may use an
> extension tag; decoders that do not understand that extension materialize it as
> an opaque extension value.

# Byte order and lengths

> r[telex.endianness]
>
> All fixed-width numeric values in telex are little-endian.

> r[telex.length.u32]
>
> Variable-length byte regions and aggregate counts use a fixed-width
> little-endian `u32` length unless a rule in this document explicitly assigns a
> different width. Length fields count bytes for byte regions and elements for
> element-counted aggregates.

> r[telex.length.canonical-width]
>
> Length and count widths are part of the canonical telex byte format.

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
> `char` is encoded as a little-endian `u32` Unicode scalar value. Values in the
> surrogate range or greater than `0x10FFFF` are invalid.

> r[telex.scalar.string]
>
> Strings are encoded as `[byte_len: u32 LE][utf8 bytes]`. The byte payload is
> valid UTF-8.

> r[telex.scalar.bytes]
>
> Byte sequences are encoded as `[byte_len: u32 LE][raw bytes]`.

> r[telex.scalar.unit]
>
> Unit encodes to zero bytes in compact mode. In self-describing mode, unit is
> represented by the unit tag with no payload.

> r[telex.scalar.never]
>
> The never type has no values.

# Aggregate encoding

> r[telex.aggregate.option]
>
> In compact mode, `Option<T>` is encoded as one byte followed by an optional
> payload: `0x00` for none, `0x01` followed by the compact `T` encoding for
> some. Any other tag is invalid. In self-describing mode, `option none` has no
> payload and `option some` is followed by one self-described value.

> r[telex.aggregate.list]
>
> Lists are variable-length ordered homogeneous sequences. The count is part of
> the value. Lists are encoded as `[count: u32 LE][element bytes...]`, with
> elements encoded in order. Element order is semantic and duplicate elements
> are valid.

> r[telex.aggregate.set]
>
> Sets are variable-length unordered homogeneous collections with unique
> elements. Sets are encoded as `[count: u32 LE][element bytes...]`. The element
> order in the byte stream is canonical, not semantic. Duplicate elements are
> invalid.
>
> - **Compact mode.** Element bytes MUST appear in ascending lexicographic
>   order of each element's compact-mode encoded byte form. This canonical
>   form is what makes two encoders produce byte-identical output for the same
>   set value and schema.
> - **Self-describing mode.** Element bytes MUST appear in ascending
>   lexicographic order of each element's complete self-described byte form,
>   including the element tag byte.

> r[telex.aggregate.map]
>
> Maps are variable-length key-value collections with unique keys. Maps are
> encoded as `[count: u32 LE][key value pairs...]`. Keys are encoded with the
> same value rules as any other value; map keys are not restricted to strings.
> Entry order in the byte stream is canonical, not semantic. Duplicate keys are
> invalid.
>
> - **Compact mode.** Entries MUST appear in ascending lexicographic order
>   of each key's compact-mode encoded byte form.
> - **Self-describing mode.** Entries MUST appear in ascending lexicographic
>   order of each key's complete self-described byte form, including the key tag
>   byte.

> r[telex.aggregate.set-map.canonical]
>
> The ordering rule for sets and maps is byte-level, not value-level. In compact
> form, the ordering key is the compact encoding of the set element or map key
> under the schema. In self-describing form, the ordering key is the complete
> self-described encoding of the set element or map key, including its tag.
> Duplicate detection uses the same ordering key: two set elements or two map
> keys with identical ordering-key bytes are duplicates.
>
> This rule applies even when an in-memory container (for example, a sorted map)
> already iterates in some natural order, because numeric, textual, or
> language-level ordering does not generally match byte ordering. An encoder may
> exploit a type-specific ordering only when it is guaranteed to yield the same
> ordering as the byte-level rule; otherwise it sorts by encoded bytes before
> emitting.

> r[telex.aggregate.set-map.decode-policy]
>
> Canonical ordering is an encoder requirement and a canonical-validation
> requirement, not an unconditional tax on every trusted decode path.
>
> A decoder that is validating Telex bytes for interchange, storage, hashing, or
> diagnostics MUST verify that set elements and map entries appear in strictly
> ascending canonical order and MUST reject noncanonical order.
>
> An implementation MAY also expose a trusted decode path that assumes the input
> was produced by a conforming encoder and skips the ordering check. A trusted
> decode path still follows the same byte grammar and scalar validity rules, but
> it MUST NOT report that the input bytes were canonical unless canonical order
> was actually verified or the bytes came directly from that implementation's
> canonical encoder.

> r[telex.aggregate.set-map.float-keys]
>
> When `f32` or `f64` appears as a set element or map key, the
> duplicate-rejection rule uses the IEEE 754 little-endian byte pattern as
> the equality predicate (and, in compact mode, the lex-ordering predicate).
> Because of that:
>
> - `NaN` bit patterns are invalid as set elements or map keys in either mode;
>   Telex set/map keys require portable equality and canonical ordering across
>   implementations.
> - Positive and negative zero have distinct byte patterns and are therefore
>   distinct set elements / map keys.
>
> Encoders that cannot guarantee NaN-freedom up-front check at encode time.
> Decoders reject any set or map whose float element/key payload is a NaN bit
> pattern.

> r[telex.aggregate.array]
>
> Arrays are fixed-shape homogeneous containers. The shape is rank `>= 1` and
> one `u64` dimension length per axis. Dimension lengths MAY be zero. Rank `1`
> with dimensions `[N]` represents a one-dimensional fixed array of `N`
> elements; rank `1` with dimensions `[0]` represents an empty one-dimensional
> fixed array. Unlike a list, an array's shape is part of the kind's type
> information rather than variable payload data.
>
> - **Compact mode.** The shape comes from the schema. The bytes are only the
>   element bytes in row-major order.
> - **Self-describing mode.** The shape is carried before the elements:
>   `[rank: u32 LE][dim_0: u64 LE]...[dim_{rank-1}: u64 LE][element bytes...]`.
>   Element bytes are self-described values in row-major order.
>
> The element count is the product of the dimensions. If the element count is
> zero, no element bytes are emitted. Rank `0` and dimension products that
> overflow `u64` are invalid.

> r[telex.aggregate.tuple]
>
> Tuples encode their elements in tuple order. Compact tuple arity comes from
> the schema. Self-describing tuples carry `[count: u32 LE]` before the
> elements. Tuple arity MUST be at least one; use `unit` for the zero-field
> product.

> r[telex.aggregate.struct.self-describing]
>
> A self-describing struct is encoded as:
>
> ```text
> tag(struct)
> field_count: u32 LE
> repeated field_count times:
>   field_name: string payload without an extra string tag
>   field_value: self-described value bytes
> ```
>
> Field names are part of the self-describing stream so the value can be
> tolerantly deserialized into an evolved local struct.

> r[telex.aggregate.struct.compact]
>
> A compact struct is encoded in the sender's declaration order. Every field is
> encoded directly:
>
> ```text
> field_value: compact value bytes
> ```
>
> The field's schema determines how many bytes the field consumes. Compact
> struct bytes do not include field names or per-field length wrappers.

> r[telex.aggregate.schema-driven-skip]
>
> A compact decoder that needs to ignore a value MUST skip it by walking the
> sender schema for that value. Compact Telex MUST NOT add an extra field or
> payload length solely to make struct fields or enum payload fields skippable.
> Variable-length primitives and aggregates still carry their own intrinsic
> lengths and counts as defined by their value grammar.

> r[telex.aggregate.enum.compact]
>
> A compact enum is encoded as `[variant_index: u32 LE][payload bytes]`, where
> `variant_index` is the sender schema's declaration index. Unit variants have
> no payload. Newtype, tuple, and struct variant payload fields are encoded
> directly according to the selected variant payload schema.

> r[telex.aggregate.enum.self-describing]
>
> A self-describing enum variant is encoded as:
>
> ```text
> tag(enum variant)
> variant_name: string payload without an extra string tag
> payload_value: self-described value bytes
> ```
>
> Unit variants use a unit payload value. Struct variants use a
> self-describing struct payload value. Tuple variants use a self-describing
> tuple payload value.

> r[telex.aggregate.dynamic-value]
>
> A dynamic value carries an arbitrary telex value whose concrete type is not
> fixed by the surrounding schema. Its content is always one self-described
> value beginning with a tag byte from `r[telex.tags]`.
>
> - **Self-describing mode.** Encoded as `0x1B [inner tag] [inner body]`. The
>   outer `0x1B` marks the field as "any type" rather than a concrete shape;
>   the inner tag and body are a regular self-described value.
> - **Compact mode.** The dynamic-value field is reached via the standard
>   schema-driven compact path, and the bytes are themselves exactly one
>   self-described value: `[inner tag] [inner body]`. No outer `0x1B` — the
>   "this field is dynamic" information comes from the schema.
>
> A dynamic-value field is the only place compact bytes contain a
> self-describing tag stream. A decoder consumes exactly one self-described
> value, then returns to the surrounding compact schema.

# Related specifications

Compact schemas and type identity are part of Telex. The companion
[Telex schemas](./schemas/) specification defines the schema model used by
compact mode and the content-hash scheme for stable schema identifiers.
