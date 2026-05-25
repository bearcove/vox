+++
title = "Telex Schema Identity"
description = "Stable content identities for schema-defined Telex types"
weight = 14
+++

This specification defines stable content identities for schema-defined types
whose values can be encoded as compact Telex values. It is separate from the
core Telex byte format: the core format defines value bytes, while this
document defines how an external schema model assigns stable identities to the
types that compact values depend on.

The hash input is a canonical schema description. It does not depend on a
programming language, reflection system, transport, or RPC protocol. Named
schema declarations carry their canonical names as part of identity; built-in
type expressions such as primitives, tuples, lists, sets, maps, arrays, and
options are identified by their structure.

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
>   * **`u32` values** (variant indices and array ranks) are fed as 4 bytes in
>     little-endian order.
>   * **TypeRef values** are fed according to `r[telex.type-id.hash.typeref]`.
>
> Implementations MUST produce identical hashes for identical canonical schema
> declarations regardless of the source language.
>
> For recursive types, see `r[telex.hash.recursive]`.

> r[telex.type-id.hash.typeref]
>
> A `TypeRef` is fed into the hasher as follows:
>
>   * **`Concrete` without args:** the tag `"concrete"` then the
>     type's content hash (8 bytes, little-endian)
>   * **`Concrete` with args:** the tag `"concrete"` then the type's
>     content hash (8 bytes, little-endian), then the tag `"args"`,
>     then each argument's `TypeRef` encoding in order (recursive)
>   * **`Var`:** the tag `"var"` then the parameter name
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
>   2. The type name (length-prefixed UTF-8 string)
>   3. The number of type parameters as a `u32` (4 bytes, LE)
>   4. Each type parameter name (length-prefixed UTF-8 string), in order
>   5. For each field, in declaration order:
>      a. The field name (length-prefixed UTF-8 string)
>      b. The field's `TypeRef` (see `r[telex.type-id.hash.typeref]`)

## Enum Hashes

> r[telex.type-id.hash.enum]
>
> To hash an enum, update the hasher with:
>
>   1. The tag `"enum"`
>   2. The type name (length-prefixed UTF-8 string)
>   3. The number of type parameters as a `u32` (4 bytes, LE)
>   4. Each type parameter name (length-prefixed UTF-8 string), in order
>   5. For each variant, in declaration order:
>      a. The variant name (length-prefixed UTF-8 string)
>      b. The variant index as a `u32` (4 bytes, little-endian)
>      c. The payload tag: `"unit"`, `"newtype"`, `"tuple"`, or `"struct"`
>      d. For newtype payloads: the inner `TypeRef`
>      e. For tuple payloads: each element's `TypeRef`, in order
>      f. For struct payloads: each field as in `r[telex.type-id.hash.struct]`
>         step 5 (name then TypeRef, in order)

## Container Hashes

> r[telex.type-id.hash.container]
>
> To hash a container type, update the hasher with:
>
>   * **List:** `"list"` then the element `TypeRef`
>   * **Set:** `"set"` then the element `TypeRef`
>   * **Option:** `"option"` then the element `TypeRef`
>   * **Array:** `"array"` then the element `TypeRef`, then the rank as a
>     `u32`, then each dimension as a `u64` in axis order
>   * **Map:** `"map"` then the key `TypeRef`, then the value `TypeRef`
>
> The kind tag string is part of the hash input even when two container kinds
> reuse a similar body grammar. This preserves the core Telex rule that value
> kind is semantic: `list<T>`, `set<T>`, and `array<T, [N]>` have distinct type
> identities.

## Tuple Hashes

> r[telex.type-id.hash.tuple]
>
> To hash a tuple, update the hasher with:
>
>   1. The tag `"tuple"`
>   2. Each element's `TypeRef`, in order

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

Example: a recursive tree type.

```text
// All strings are length-prefixed: len(s) as u32 LE, then UTF-8 bytes.
// L("foo") = 03 00 00 00 "foo"
//
// Step 1: preliminary hash
//   TreeNode: blake3(L("struct") || L("label") || hash(string)
//                    || L("children") || hash_of(list, SENTINEL))
//   => preliminary_hash = 0xABCD...
//
// Step 2: deduplication (only one type, nothing to dedup)
//
// Step 3: canonical order (only one type, so trivial)
//   [TreeNode]
//
// Step 4: final hash
//   group_hash = blake3(preliminary_hash)[0..8]
//   TreeNode.type_id = blake3(group_hash || 0u64)[0..8]
```

Example: mutually recursive types.

```text
// Expr { body: ExprBody }
// ExprBody { Literal(u64), Add(Expr, Expr) }
//
// Step 1: preliminary hashes (recursive refs become SENTINEL)
//   Expr:     blake3(L("struct") || L("body") || SENTINEL)         => 0x1111...
//   ExprBody: blake3(L("enum") || L("Literal") || 0u32 || L("newtype") || hash(u64)
//                    || L("Add") || 1u32 || L("struct") || L("left") || SENTINEL
//                    || L("right") || SENTINEL)                    => 0x2222...
//
// Step 2: deduplication (both canonical declarations are distinct, nothing to dedup)
//
// Step 3: canonical order (sort by preliminary hash)
//   [Expr (0x1111), ExprBody (0x2222)]
//
// Step 4: final hashes
//   group_hash = blake3(0x1111... || 0x2222...)[0..8]
//   Expr.type_id     = blake3(group_hash || 0u64)[0..8]
//   ExprBody.type_id = blake3(group_hash || 1u64)[0..8]
```
