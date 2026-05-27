// binette compact encode/decode for Vox schema payloads.

import type {
  SchemaPayload,
  Schema,
  SchemaKind,
  TypeRef,
  VariantPayload,
  FieldSchema,
  VariantSchema,
  PrimitiveType,
  ChannelDirection,
} from "@bearcove/binette";
import { decodeWithTypeRef, encodeWithTypeRef } from "@bearcove/binette";
import { schemaPayloadRootRef, schemaPayloadSchemaRegistry } from "@bearcove/vox-wire";

type RustTypeRef =
  | { tag: "Concrete"; type_id: bigint; args: RustTypeRef[] }
  | { tag: "Var"; name: string };

type RustPrimitiveType = { tag: Capitalize<PrimitiveType> };
type RustChannelDirection = { tag: "Tx" | "Rx" };

type RustFieldSchema = {
  name: string;
  type_ref: RustTypeRef;
  required: boolean;
};

type RustVariantPayload =
  | { tag: "Unit" }
  | { tag: "Newtype"; type_ref: RustTypeRef }
  | { tag: "Tuple"; types: RustTypeRef[] }
  | { tag: "Struct"; fields: RustFieldSchema[] };

type RustVariantSchema = {
  name: string;
  index: number;
  payload: RustVariantPayload;
};

type RustSchemaKind =
  | { tag: "Struct"; name: string; fields: RustFieldSchema[] }
  | { tag: "Enum"; name: string; variants: RustVariantSchema[] }
  | { tag: "Tuple"; elements: RustTypeRef[] }
  | { tag: "List"; element: RustTypeRef }
  | { tag: "Map"; key: RustTypeRef; value: RustTypeRef }
  | { tag: "Array"; element: RustTypeRef; length: bigint }
  | { tag: "Option"; element: RustTypeRef }
  | { tag: "Channel"; direction: RustChannelDirection; element: RustTypeRef }
  | { tag: "Primitive"; primitive_type: RustPrimitiveType };

type RustSchema = {
  id: bigint;
  type_params: string[];
  kind: RustSchemaKind;
};

type RustSchemaPayload = {
  schemas: RustSchema[];
  root: RustTypeRef;
};

const PRIMITIVE_TO_RUST = {
  bool: "Bool",
  u8: "U8",
  u16: "U16",
  u32: "U32",
  u64: "U64",
  u128: "U128",
  i8: "I8",
  i16: "I16",
  i32: "I32",
  i64: "I64",
  i128: "I128",
  f32: "F32",
  f64: "F64",
  char: "Char",
  string: "String",
  unit: "Unit",
  never: "Never",
  bytes: "Bytes",
  payload: "Payload",
} as const satisfies Record<PrimitiveType, RustPrimitiveType["tag"]>;

const PRIMITIVE_FROM_RUST = new Map(
  Object.entries(PRIMITIVE_TO_RUST).map(([key, value]) => [value, key as PrimitiveType]),
);

export function decodeSchemaPayload(bytes: Uint8Array): SchemaPayload {
  const decoded = decodeWithTypeRef(bytes, 0, schemaPayloadRootRef, schemaPayloadSchemaRegistry);
  if (decoded.next !== bytes.length) {
    throw new Error(`schema payload: trailing ${bytes.length - decoded.next} bytes`);
  }
  return schemaPayloadFromRust(decoded.value as RustSchemaPayload);
}

export function encodeSchemaPayload(payload: SchemaPayload): Uint8Array {
  return encodeWithTypeRef(schemaPayloadToRust(payload), schemaPayloadRootRef, schemaPayloadSchemaRegistry);
}

export function normalizeSchemaList(value: unknown): Schema[] {
  return Array.isArray(value) ? (value as Schema[]).map(normalizeSchema) : [];
}

export function normalizeSchema(schema: Schema): Schema {
  return {
    ...schema,
    kind: normalizeSchemaKind(schema.kind),
  };
}

function schemaPayloadToRust(payload: SchemaPayload): RustSchemaPayload {
  return {
    schemas: payload.schemas.map(schemaToRust),
    root: typeRefToRust(payload.root),
  };
}

function schemaPayloadFromRust(payload: RustSchemaPayload): SchemaPayload {
  return {
    schemas: payload.schemas.map(schemaFromRust),
    root: typeRefFromRust(payload.root),
  };
}

export function schemaListToRust(schemas: Schema[]): RustSchema[] {
  return schemas.map(schemaToRust);
}

export function schemaListFromRust(schemas: RustSchema[]): Schema[] {
  return schemas.map(schemaFromRust);
}

function schemaToRust(schema: Schema): RustSchema {
  return {
    id: schema.id,
    type_params: schema.type_params,
    kind: schemaKindToRust(schema.kind),
  };
}

function schemaFromRust(schema: RustSchema): Schema {
  return {
    id: schema.id,
    type_params: schema.type_params,
    kind: schemaKindFromRust(schema.kind),
  };
}

function typeRefToRust(ref_: TypeRef): RustTypeRef {
  switch (ref_.tag) {
    case "concrete":
      return {
        tag: "Concrete",
        type_id: ref_.type_id,
        args: ref_.args.map(typeRefToRust),
      };
    case "var":
      return { tag: "Var", name: ref_.name };
  }
}

function typeRefFromRust(ref_: RustTypeRef): TypeRef {
  switch (ref_.tag) {
    case "Concrete":
      return {
        tag: "concrete",
        type_id: ref_.type_id,
        args: ref_.args.map(typeRefFromRust),
      };
    case "Var":
      return { tag: "var", name: ref_.name };
  }
}

function fieldToRust(field: FieldSchema): RustFieldSchema {
  return {
    name: field.name,
    type_ref: typeRefToRust(field.type_ref),
    required: field.required,
  };
}

function fieldFromRust(field: RustFieldSchema): FieldSchema {
  return {
    name: field.name,
    type_ref: typeRefFromRust(field.type_ref),
    required: field.required,
  };
}

function variantToRust(variant: VariantSchema): RustVariantSchema {
  return {
    name: variant.name,
    index: variant.index,
    payload: variantPayloadToRust(variant.payload),
  };
}

function variantFromRust(variant: RustVariantSchema): VariantSchema {
  return {
    name: variant.name,
    index: variant.index,
    payload: variantPayloadFromRust(variant.payload),
  };
}

function schemaKindToRust(kind: SchemaKind): RustSchemaKind {
  switch (kind.tag) {
    case "struct":
      return { tag: "Struct", name: kind.name, fields: kind.fields.map(fieldToRust) };
    case "enum":
      return { tag: "Enum", name: kind.name, variants: kind.variants.map(variantToRust) };
    case "tuple":
      return { tag: "Tuple", elements: kind.elements.map(typeRefToRust) };
    case "list":
      return { tag: "List", element: typeRefToRust(kind.element) };
    case "map":
      return { tag: "Map", key: typeRefToRust(kind.key), value: typeRefToRust(kind.value) };
    case "array":
      return { tag: "Array", element: typeRefToRust(kind.element), length: BigInt(kind.length) };
    case "option":
      return { tag: "Option", element: typeRefToRust(kind.element) };
    case "channel":
      return {
        tag: "Channel",
        direction: { tag: kind.direction === "tx" ? "Tx" : "Rx" },
        element: typeRefToRust(kind.element),
      };
    case "primitive":
      return { tag: "Primitive", primitive_type: { tag: PRIMITIVE_TO_RUST[kind.primitive_type] } };
  }
}

function schemaKindFromRust(kind: RustSchemaKind): SchemaKind {
  switch (kind.tag) {
    case "Struct":
      return { tag: "struct", name: kind.name, fields: kind.fields.map(fieldFromRust) };
    case "Enum":
      return { tag: "enum", name: kind.name, variants: kind.variants.map(variantFromRust) };
    case "Tuple":
      return { tag: "tuple", elements: kind.elements.map(typeRefFromRust) };
    case "List":
      return { tag: "list", element: typeRefFromRust(kind.element) };
    case "Map":
      return { tag: "map", key: typeRefFromRust(kind.key), value: typeRefFromRust(kind.value) };
    case "Array":
      return { tag: "array", element: typeRefFromRust(kind.element), length: Number(kind.length) };
    case "Option":
      return { tag: "option", element: typeRefFromRust(kind.element) };
    case "Channel":
      return {
        tag: "channel",
        direction: channelDirectionFromRust(kind.direction),
        element: typeRefFromRust(kind.element),
      };
    case "Primitive":
      return { tag: "primitive", primitive_type: primitiveFromRust(kind.primitive_type) };
  }
}

function variantPayloadToRust(payload: VariantPayload): RustVariantPayload {
  switch (payload.tag) {
    case "unit":
      return { tag: "Unit" };
    case "newtype":
      return { tag: "Newtype", type_ref: typeRefToRust(payload.type_ref) };
    case "tuple":
      return { tag: "Tuple", types: payload.types.map(typeRefToRust) };
    case "struct":
      return { tag: "Struct", fields: payload.fields.map(fieldToRust) };
  }
}

function variantPayloadFromRust(payload: RustVariantPayload): VariantPayload {
  switch (payload.tag) {
    case "Unit":
      return { tag: "unit" };
    case "Newtype":
      return { tag: "newtype", type_ref: typeRefFromRust(payload.type_ref) };
    case "Tuple":
      return { tag: "tuple", types: payload.types.map(typeRefFromRust) };
    case "Struct":
      return { tag: "struct", fields: payload.fields.map(fieldFromRust) };
  }
}

function primitiveFromRust(primitive: RustPrimitiveType): PrimitiveType {
  const value = PRIMITIVE_FROM_RUST.get(primitive.tag);
  if (value === undefined) {
    throw new Error(`unknown primitive type ${primitive.tag}`);
  }
  return value;
}

function channelDirectionFromRust(direction: RustChannelDirection): ChannelDirection {
  switch (direction.tag) {
    case "Tx":
      return "tx";
    case "Rx":
      return "rx";
  }
}

function normalizeSchemaKind(kind: SchemaKind): SchemaKind {
  switch (kind.tag) {
    case "enum":
      return {
        ...kind,
        variants: kind.variants.map((variant) => ({
          ...variant,
          payload: normalizeVariantPayload(variant.payload),
        })),
      };
    default:
      return kind;
  }
}

function normalizeVariantPayload(payload: VariantPayload | "unit"): VariantPayload {
  if (payload === "unit") {
    return { tag: "unit" };
  }
  return payload;
}
