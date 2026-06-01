//! Per-service phon schema emission for the Swift runtime.
//!
//! Mirrors `targets/typescript/phon.rs` but emits Swift: the `{service}Methods`
//! table of `PhonMethodSchemas` (args/response roots + closures + channel metadata)
//! AND — what the Swift typed path needs beyond TS — a `Descriptor` per method's
//! args tuple and response wire type (`Result<T, VoxError<E>>`), plus the merged
//! `{service}Registry`.

use facet_core::Shape;
use heck::ToLowerCamelCase;
use vox_types::{ServiceDescriptor, ShapeKind, classify_shape, is_rx, is_tx};

use super::phon_descriptor::descriptor_expr;
use crate::render::hex_u64;

/// The ok (success) type behind a method's declared return type.
fn ok_shape(return_shape: &'static Shape) -> &'static Shape {
    match classify_shape(return_shape) {
        ShapeKind::Result { ok, .. } => ok,
        _ => return_shape,
    }
}

/// The content-derived phon root id for a shape.
fn root_id(shape: &'static Shape) -> u64 {
    vox_phon::schema_id_for_shape(shape)
        .expect("phon schema id")
        .0
}

/// A shape's args schema closure bytes as a Swift `[UInt8]` literal body.
fn closure_bytes(shape: &'static Shape) -> String {
    let bytes = vox_phon::schema_bytes_for_shape(shape).expect("phon schema bytes");
    bytes
        .iter()
        .map(u8::to_string)
        .collect::<Vec<_>>()
        .join(", ")
}

/// The wire error type `VoxError<E>` (`Result<T, VoxError<E>>` is every method's
/// response) + the empty `Infallible` (the `E` of an infallible method). Generated
/// once; the per-method response descriptors materialize them on the typed path.
pub fn generate_wire_error_types() -> String {
    let mut out = String::new();
    out.push_str("// MARK: - wire error type\n\n");
    // `Infallible`: Rust `core::convert::Infallible` (uninhabited) — an infallible
    // method's `User(E)` arm is never constructed.
    out.push_str("public enum Infallible: Sendable {}\n\n");
    out.push_str("/// The wire error of `Result<T, VoxError<E>>`. Variant order matches the\n");
    out.push_str("/// Rust `VoxError<E>` (User=0 … Indeterminate=7) so wire indices align.\n");
    out.push_str("public enum VoxError<E: Sendable>: Error, Sendable {\n");
    out.push_str("    case user(E)\n");
    out.push_str("    case unknownMethod\n");
    out.push_str("    case invalidPayload(String)\n");
    out.push_str("    case cancelled\n");
    out.push_str("    case connectionClosed\n");
    out.push_str("    case sessionShutdown\n");
    out.push_str("    case sendFailed\n");
    out.push_str("    case indeterminate\n");
    out.push_str("}\n\n");
    out
}

/// Generate the `{service}` phon registry + per-method schema table + descriptors.
pub fn generate_phon_service(service: &ServiceDescriptor) -> String {
    let name = service.service_name.to_lower_camel_case();
    let mut out = String::new();

    out.push_str(&generate_wire_error_types());
    out.push_str(
        "// MARK: - phon service schemas (registry + per-method roots/descriptors/channels)\n\n",
    );

    // Per-method descriptor globals (the args tuple + the `Result<T, VoxError<E>>`
    // wire type). Built once; immutable — `nonisolated(unsafe)` like the envelope.
    for m in service.methods {
        let mname = m.method_name.to_lower_camel_case();
        out.push_str(&format!(
            "nonisolated(unsafe) let {name}_{mname}_ArgsDescriptor: Descriptor = {}\n",
            descriptor_expr(m.args_shape)
        ));
        out.push_str(&format!(
            "nonisolated(unsafe) let {name}_{mname}_ResponseDescriptor: Descriptor = {}\n",
            descriptor_expr(m.response_wire_shape)
        ));
    }
    out.push('\n');

    out.push_str(&format!(
        "public let {name}Methods: [UInt64: PhonMethodSchemas] = [\n"
    ));
    for m in service.methods {
        let mname = m.method_name.to_lower_camel_case();
        let method_id = crate::method_id(m);
        let args_root = root_id(m.args_shape);
        let ok_root = root_id(ok_shape(m.return_shape));
        let response_root = root_id(m.response_wire_shape);
        let args_closure = closure_bytes(m.args_shape);
        let response_closure = closure_bytes(m.response_wire_shape);

        out.push_str(&format!("    {}: PhonMethodSchemas(\n", hex_u64(method_id)));
        out.push_str(&format!(
            "        argsRoot: SchemaId({}),\n",
            hex_u64(args_root)
        ));
        out.push_str(&format!("        argsSchemaClosure: [{args_closure}],\n"));
        out.push_str(&format!(
            "        argsDescriptor: {name}_{mname}_ArgsDescriptor,\n"
        ));
        out.push_str(&format!(
            "        okRoot: SchemaId({}),\n",
            hex_u64(ok_root)
        ));
        out.push_str(&format!(
            "        responseRoot: SchemaId({}),\n",
            hex_u64(response_root)
        ));
        out.push_str(&format!(
            "        responseSchemaClosure: [{response_closure}],\n"
        ));
        out.push_str(&format!(
            "        responseDescriptor: {name}_{mname}_ResponseDescriptor,\n"
        ));
        out.push_str("        channels: [");
        let mut first = true;
        for (i, arg) in m.args.iter().enumerate() {
            let dir = if is_tx(arg.shape) {
                Some(true)
            } else if is_rx(arg.shape) {
                Some(false)
            } else {
                None
            };
            if let Some(is_tx_dir) = dir {
                let element = arg
                    .channel_element
                    .expect("Tx/Rx arg must carry its channel element shape");
                if !first {
                    out.push_str(", ");
                }
                first = false;
                out.push_str(&format!(
                    "PhonChannelMeta(index: {i}, isTx: {is_tx_dir}, elementRoot: SchemaId({}))",
                    hex_u64(root_id(element))
                ));
            }
        }
        out.push_str("]),\n");
    }
    out.push_str("]\n\n");

    out.push_str(&format!(
        "nonisolated(unsafe) public let {name}Registry: Registry = buildServiceRegistry({name}Methods)\n\n"
    ));

    // Per-method lowered programs (cached, init-once). Encode uses `lowerTyped` (own
    // schema); decode reconciles writer→reader via `lowerDecode`. The client encodes
    // args + decodes the response; the server decodes args + encodes the response.
    out.push_str("// MARK: - per-method lowered programs\n\n");
    for m in service.methods {
        let mname = m.method_name.to_lower_camel_case();
        for (suffix, fn_call) in [
            ("ArgsEncodeProgram", "lowerTyped"),
            ("ArgsDecodeProgram", "lowerDecode"),
            ("ResponseEncodeProgram", "lowerTyped"),
            ("ResponseDecodeProgram", "lowerDecode"),
        ] {
            let desc = if suffix.starts_with("Args") {
                format!("{name}_{mname}_ArgsDescriptor")
            } else {
                format!("{name}_{mname}_ResponseDescriptor")
            };
            out.push_str(&format!(
                "nonisolated(unsafe) let {name}_{mname}_{suffix}: MemProgram = try! {fn_call}({desc}, {name}Registry)\n"
            ));
        }
    }
    out.push('\n');
    out
}

/// The name of the Swift descriptor/program globals for a method's args/response.
pub fn method_global_prefix(service_name: &str, method_name: &str) -> String {
    use heck::ToLowerCamelCase;
    format!(
        "{}_{}",
        service_name.to_lower_camel_case(),
        method_name.to_lower_camel_case()
    )
}
