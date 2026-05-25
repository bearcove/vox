+++
title = "Telex Schemas"
description = "Schema model and stable type identities for compact Telex values"
weight = 14
+++

Compact Telex bytes are schema-driven. A compact value does not carry its own
tag, field names, variant names, type names, or fixed array shape; those facts
come from a Telex schema known to the decoder.

This specification defines the Telex schema model and the stable content
identities assigned to schema-defined types. It is part of Telex itself:
self-describing Telex can be decoded using only tags, while compact Telex needs
schemas to exist at all.

The schema model is language-independent. A programming language, reflection
system, IDL, RPC protocol, or transport may produce Telex schemas, exchange
them, cache them, or attach extra metadata, but the compact Telex byte grammar
depends only on the Telex schema model defined here.

# Schema model

> r[telex.schema.model]
>
> A Telex schema describes one compact-capable Telex type. A schema contains:
>
> - `id`: the `u64` type ID defined by `r[telex.type-id]`
> - `type_params`: ordered type parameter names for generic declarations
> - `kind`: one of the schema kinds defined by `r[telex.schema.kinds]`
>
> The schema's `id` is derived from its canonical content. A received schema
> whose declared `id` does not match its content is invalid.

> r[telex.schema.type-ref]
>
> A type reference inside a schema is one of:
>
> - `concrete(type_id, args...)`: a reference to a concrete type declaration,
>   with zero or more type arguments
> - `var(name)`: a reference to one of the enclosing schema's `type_params`
>
> Type arguments are themselves type references. A `var(name)` reference is
> valid only when `name` appears in the nearest enclosing schema's
> `type_params`.

> r[telex.schema.kinds]
>
> Telex defines these schema kinds:
>
> | Kind | Contents |
> |------|----------|
> | primitive | one primitive type from `r[telex.schema.primitive]` |
> | struct | canonical declaration name and ordered fields |
> | enum | canonical declaration name and ordered variants |
> | tuple | ordered fixed-arity elements |
> | list | homogeneous element type |
> | set | homogeneous element type |
> | map | key type and value type |
> | array | homogeneous element type and one or more dimensions |
> | option | element type |
> | dynamic value | nested self-described Telex value |

> r[telex.schema.primitive]
>
> Primitive schema kinds are: `bool`, `u8`, `u16`, `u32`, `u64`, `u128`, `i8`,
> `i16`, `i32`, `i64`, `i128`, `f32`, `f64`, `char`, `string`, `unit`,
> `never`, `bytes`, and `payload`.
>
> Primitive type IDs are well-known constants defined by
> `r[telex.type-id.hash.primitives]`. A schema registry does not need to carry
> primitive schema declarations before those primitive IDs can be referenced.

> r[telex.schema.name]
>
> Struct and enum schemas carry a canonical declaration name. The canonical
> declaration name is a non-empty UTF-8 string chosen by the schema producer.
> Telex does not assign package, module, crate, or namespace prefixes.
>
> By default, a source-language mapping should use the declaration's local type
> name. A mapping may provide an explicit schema-name override when a producer
> wants a more stable or more globally distinctive name. Two named declarations
> with the same canonical declaration name, type parameters, and structure have
> the same Telex type identity.

> r[telex.schema.fields]
>
> A struct field contains a field name and a type reference. Struct fields are
> listed in declaration order; that order is the compact serialization order.
> Field names are used by schema-aware consumers for matching and diagnostics,
> but compact struct bytes contain only field values.
>
> Enum variants contain a variant name, a `u32` compact variant index, and one
> payload: unit, newtype, tuple, or struct. Tuple and struct payloads contain
> the same type references and fields described above.

> r[telex.schema.array]
>
> An array schema contains an element type reference and a non-empty list of
> `u64` dimensions. Dimension values MAY be zero. The rank is the dimension
> list length; rank `0` is invalid.

> r[telex.schema.tuple]
>
> A tuple schema contains one or more element type references. Use the `unit`
> primitive for the zero-field product.

> r[telex.schema.dynamic]
>
> A dynamic-value schema kind has no child types. Compact bytes for that schema
> kind are exactly one self-described Telex value as defined by
> `r[telex.aggregate.dynamic-value]`.

> r[telex.schema.extension]
>
> `extension` is not a compact schema kind. Extension values arise only when
> self-describing decoding encounters an extension tag as defined by
> `r[telex.tags.extension]`, including inside a dynamic-value field.

> r[telex.schema.encoding.self-describing]
>
> Telex schemas are encoded for interchange as self-describing Telex values.
> A schema decoder first decodes a generic self-described Telex value, then
> materializes that value into the schema model in this document.
>
> Because schema interchange uses self-describing Telex, decoding a schema does
> not require a pre-existing schema for the schema type.

> r[telex.schema.registry+2]
>
> A schema registry maps type IDs to schemas. Before installing non-primitive
> schemas into a registry, a consumer MUST verify the declared IDs against the
> schemas' canonical content.
>
> Schema references may point to schemas already in the registry or to other
> schemas being installed in the same batch. Batch order is not significant:
> a consumer first indexes the batch by declared type ID, then resolves
> references against the combined existing registry and batch. Duplicate
> non-identical declarations for the same type ID are invalid.
>
> Verification is performed over the reference graph of the batch being
> installed. Strongly connected components are verified in dependency order:
>
> - A non-recursive singleton is verified by recomputing its ID with
>   `r[telex.type-id.hash]` and comparing the result with its declared `id`.
>   References to earlier components in the same batch use those components'
>   newly verified IDs.
> - A self-recursive schema or mutually recursive group is verified as one
>   strongly connected component using `r[telex.hash.recursive]`; every computed
>   final ID in the component must match the corresponding declared `id`.
> - References to schemas already present in the registry use those schemas'
>   verified IDs as ordinary external references.

# Schema encoding

> r[telex.schema.format+2]
>
> A Telex schema encoded for interchange is a self-described Telex struct with
> exactly these fields, emitted in this order by canonical encoders:
>
> - `id`: `u64`
> - `type_params`: list of UTF-8 strings, empty for non-generic types
> - `kind`: one schema-kind payload from `r[telex.schema.format.kind+2]`
>
> Decoders identify fields by name using the self-describing struct rules in
> `r[telex.aggregate.struct.self-describing]`. Canonical schema encoders MUST
> NOT emit extra fields. A schema value with missing, duplicate, or extra fields
> is not a valid core Telex schema encoding.
>
> Protocols that attach metadata to Telex schemas MUST carry that metadata
> outside this core schema value. Extra protocol metadata is not part of the
> Telex schema content and MUST NOT affect `r[telex.type-id.hash]`.

> r[telex.schema.format.type-ref+2]
>
> A type reference is encoded as a self-described enum variant:
>
> - variant `concrete`: self-described struct payload with fields `type_id:
>   u64` and `args: list<type_ref>`, emitted in that order by canonical
>   encoders
> - variant `var`: UTF-8 string payload naming one of the enclosing schema's
>   type parameters
>
> The `args` list is empty for non-generic concrete references.

> r[telex.schema.format.kind+2]
>
> A schema kind is encoded as a self-described enum variant:
>
> - `primitive`: primitive tag string from `r[telex.schema.primitive]`
> - `struct`: self-described struct payload with fields `name: string` and
>   `fields: list<field>`
> - `enum`: self-described struct payload with fields `name: string` and
>   `variants: list<variant>`
> - `tuple`: non-empty `list<type_ref>`
> - `list`: self-described struct payload with field `element: type_ref`
> - `set`: self-described struct payload with field `element: type_ref`
> - `map`: self-described struct payload with fields `key: type_ref` and
>   `value: type_ref`
> - `array`: self-described struct payload with fields `element: type_ref` and
>   `dimensions: non-empty list<u64>`
> - `option`: self-described struct payload with field `element: type_ref`
> - `dynamic`: unit payload
>
> Canonical encoders emit payload fields in the order listed above.

> r[telex.schema.format.fields+2]
>
> A field descriptor is a self-described struct containing exactly these fields,
> emitted in this order by canonical encoders:
>
> - `name`: UTF-8 field name
> - `type_ref`: type reference
>
> Field descriptors appear in compact serialization order.

> r[telex.schema.format.variants+2]
>
> A variant descriptor is a self-described struct containing exactly these
> fields, emitted in this order by canonical encoders:
>
> - `name`: UTF-8 variant name
> - `index`: `u32` compact variant index
> - `payload`: a self-described enum variant, one of:
>   - `unit`: unit payload
>   - `newtype`: type-reference payload
>   - `tuple`: list of type references
>   - `struct`: list of field descriptors
>
> Variant descriptors appear in declaration order. The `index` field is the
> compact byte value used by `r[telex.aggregate.enum.compact]`.

# Type identity

> r[telex.type-id]
>
> A type ID is a `u64` content hash: a deterministic hash of a canonical schema
> declaration or built-in type expression. For named declarations, the canonical
> declaration name is part of the hash input. For generic declarations, the hash
> is of the declaration, with type variable slots, not of any specific
> instantiation. The same declaration always produces the same hash regardless
> of which connection, session, process, or language produced it. On the wire,
> a type ID is encoded as a little-endian `u64`.

> r[telex.type-id.context-free]
>
> Type identity is context-free. A type's hash MUST NOT change based on whether
> the type appears as a method argument, response, struct field, enum payload,
> collection element, or any other use site.
>
> A schema producer maps a source-language type to its canonical Telex schema
> form before hashing. If that source type is a named schema declaration, its
> canonical name is part of identity. If that source type is a transparent alias
> or transparent wrapper, the alias or wrapper is erased before hashing and the
> inner type identity is used. Telex does not infer transparency or nominality
> from use-site position.

> r[telex.type-id.hash]
>
> The content hash of a type declaration is computed by feeding a canonical
> byte sequence into blake3, then taking the first 8 bytes of the output as a
> little-endian `u64`. The canonical byte sequence is constructed by updating
> the hasher with the components described below.
>
>   * **Strings** (declaration names, field names, variant names, tag strings,
>     type parameter names) are fed as their byte length as a `u32` in
>     little-endian order, followed by the raw UTF-8 bytes. The length prefix
>     ensures the encoding is injective: no two different type structures
>     produce the same byte sequence.
>   * **`u64` values** (array dimensions) are fed as 8 bytes in
>     little-endian order.
>   * **`u32` values** (sequence counts, variant indices, and array ranks) are
>     fed as 4 bytes in little-endian order.
>   * **Type references** are fed according to `r[telex.type-id.hash.typeref]`.
>
> Implementations MUST produce identical hashes for identical canonical schema
> declarations regardless of the source language.
>
> For recursive types, see `r[telex.hash.recursive]`.

> r[telex.type-id.hash.typeref]
>
> A type reference is fed into the hasher as follows:
>
>   * **Concrete without args:** the tag `"concrete"` then the
>     type's content hash (8 bytes, little-endian)
>   * **Concrete with args:** the tag `"concrete"` then the type's
>     content hash (8 bytes, little-endian), then the tag `"args"`,
>     then the argument count as a `u32`, then each argument's type-reference
>     encoding in order (recursive)
>   * **Type variable:** the tag `"var"` then the parameter name
>     (length-prefixed UTF-8 string)

## Primitive Type Hashes

The hash input for a primitive type is a single tag string. A schema mapping may
define transparent aliases or wrappers that flatten to an inner type's hash.
Ordinary named wrappers do not flatten: a single-field wrapper declaration
hashes as its declared product type, not as the wrapped primitive. Flattening is
opt-in so a schema mapping can erase wrapper identity when that is intended.

> r[telex.type-id.hash.primitives]
>
> The hash of a primitive type is `blake3(len(tag) || tag)[0..8]` where
> `len(tag)` is the tag's byte length as a `u32` LE, and `tag` is one
> of the following UTF-8 strings:
>
> | Compact type | Tag string |
> |--------------|------------|
> | bool          | `"bool"`   |
> | u8            | `"u8"`     |
> | u16           | `"u16"`    |
> | u32           | `"u32"`    |
> | u64           | `"u64"`    |
> | u128          | `"u128"`   |
> | i8            | `"i8"`     |
> | i16           | `"i16"`    |
> | i32           | `"i32"`    |
> | i64           | `"i64"`    |
> | i128          | `"i128"`   |
> | f32           | `"f32"`    |
> | f64           | `"f64"`    |
> | char          | `"char"`   |
> | string        | `"string"` |
> | unit          | `"unit"`   |
> | never         | `"never"`  |
> | bytes         | `"bytes"`   |
> | payload       | `"payload"` |
>
> These 19 hashes are constants. Implementations MAY precompute them.

## Struct Hashes

> r[telex.type-id.hash.struct]
>
> To hash a struct, update the hasher with:
>
>   1. The tag `"struct"`
>   2. The canonical declaration name (length-prefixed UTF-8 string)
>   3. The number of type parameters as a `u32` (4 bytes, LE)
>   4. Each type parameter name (length-prefixed UTF-8 string), in order
>   5. The number of fields as a `u32` (4 bytes, LE)
>   6. For each field, in declaration order:
>      a. The field name (length-prefixed UTF-8 string)
>      b. The field's type reference (see `r[telex.type-id.hash.typeref]`)

## Enum Hashes

> r[telex.type-id.hash.enum]
>
> To hash an enum, update the hasher with:
>
>   1. The tag `"enum"`
>   2. The canonical declaration name (length-prefixed UTF-8 string)
>   3. The number of type parameters as a `u32` (4 bytes, LE)
>   4. Each type parameter name (length-prefixed UTF-8 string), in order
>   5. The number of variants as a `u32` (4 bytes, LE)
>   6. For each variant, in declaration order:
>      a. The variant name (length-prefixed UTF-8 string)
>      b. The variant index as a `u32` (4 bytes, little-endian)
>      c. The payload tag: `"unit"`, `"newtype"`, `"tuple"`, or `"struct"`
>      d. For newtype payloads: the inner type reference
>      e. For tuple payloads: the element count as a `u32`, then each
>         element's type reference, in order
>      f. For struct payloads: each field as in `r[telex.type-id.hash.struct]`
>         steps 5-6 (field count, then name and type reference in order)

## Container Hashes

> r[telex.type-id.hash.container]
>
> To hash a container type, update the hasher with:
>
>   * **List:** `"list"` then the element type reference
>   * **Set:** `"set"` then the element type reference
>   * **Option:** `"option"` then the element type reference
>   * **Array:** `"array"` then the element type reference, then the rank as a
>     `u32`, then each dimension as a `u64` in axis order
>   * **Map:** `"map"` then the key type reference, then the value type reference
>
> The kind tag string is part of the hash input even when two container kinds
> reuse a similar body grammar. This preserves the core Telex rule that value
> kind is semantic: `list<T>`, `set<T>`, and `array<T, [N]>` have distinct type
> identities.

## Dynamic-Value Hashes

> r[telex.type-id.hash.dynamic]
>
> To hash the dynamic-value schema kind, update the hasher with the tag
> `"dynamic"`. Dynamic values have no child type references in the schema; the
> concrete value kind is carried by each nested self-described value.

## Tuple Hashes

> r[telex.type-id.hash.tuple]
>
> To hash a tuple, update the hasher with:
>
>   1. The tag `"tuple"`
>   2. The element count as a `u32`
>   3. Each element's type reference, in order

Content hashes give type IDs a universal meaning. A peer that receives a schema
tagged with a content hash it has already seen knows it already has that
schema, regardless of which connection, process, or store supplied it.

# Hashing Recursive Types

Non-recursive types have straightforward content hashes: hash the structure,
reference child types by their hashes. Recursive types create a cycle: the hash
of `TreeNode` depends on the hash of `list<TreeNode>`, which depends on the
hash of `TreeNode`.

The solution is a four-step algorithm that computes preliminary hashes to
establish a canonical ordering, then derives final hashes from that ordering.

> r[telex.hash.recursive]
>
> To compute content hashes for a mutually recursive group of types:
>
>   1. **Preliminary hashes.** Hash each type in the group using the
>      normal rules (see `r[telex.type-id.hash]`), except that any
>      reference to another type in the same recursive group is replaced
>      with 8 zero bytes (the **sentinel**). References to types outside
>      the group use their real content hashes as normal. The result is
>      one preliminary hash per type.
>
>   2. **Deduplication.** If two entries in the group have identical
>      canonical byte sequences (the full input to blake3 from step 1),
>      they are the same canonical declaration and MUST be deduplicated:
>      collapsed to a single entry before proceeding. This does not collapse
>      different named declarations that happen to have the same compact byte
>      shape; their canonical declaration names are part of the byte sequence.
>
>   3. **Canonical ordering.** Sort the (now-unique) types by their
>      preliminary hash (ascending, unsigned integer comparison). In the
>      unlikely event that two types have the same preliminary hash but
>      different canonical byte sequences (a 64-bit collision), break the
>      tie by lexicographic comparison of their canonical byte sequences.
>
>   4. **Final hashes.** Compute the **group hash** as
>      `blake3(preliminary_hash_0 || preliminary_hash_1 || ...)[0..8]`
>      where the preliminary hashes are concatenated in canonical order.
>      Then each type's final content hash is
>      `blake3(group_hash || index)[0..8]` where `index` is the type's
>      position in the canonical order, encoded as a `u64` in
>      little-endian order.
>
> These final hashes are the types' content IDs: plain `u64` values,
> indistinguishable from non-recursive type hashes. No special
> representation is needed on the wire or in data structures.

> r[telex.hash.recursive.non-recursive]
>
> A non-recursive type does not participate in this algorithm. Its
> content hash is computed directly from its structure as described
> in `r[telex.type-id.hash]`.
