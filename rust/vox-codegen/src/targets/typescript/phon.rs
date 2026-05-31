//! Per-service phon schema emission for the TypeScript runtime.
//!
//! Emits a phon `Registry` covering every method's args + ok-type schemas, plus a
//! per-method table the runtime needs: the args root id, the args schema-closure
//! bytes to advertise in the `schemas:` field, the ok root id, and channel
//! metadata (which arg is a `Tx`/`Rx`, its direction, and its element root) —
//! since channels are opaque on the wire (`r[rpc.channel.payload-encoding]`).

use facet_core::{Facet, Shape};
use vox_types::{ServiceDescriptor, ShapeKind, classify_shape, is_rx, is_tx};

use crate::render::hex_u64;

/// The ok (success) type shape behind a method's declared return type.
fn ok_shape(return_shape: &'static Shape) -> &'static Shape {
    match classify_shape(return_shape) {
        ShapeKind::Result { ok, .. } => ok,
        _ => return_shape,
    }
}

/// The content-derived phon root id for a single shape.
fn root_id(shape: &'static Shape) -> u64 {
    phon_codegen::Module::from_shapes(&[shape])
        .expect("derive phon schema")
        .roots[0]
        .id
        .0
}

/// The args schema closure bytes (vox-phon framing) as a hex string — what the
/// caller advertises in `RequestCall.schemas`.
fn args_closure_hex(shape: &'static Shape) -> String {
    let bytes = vox_phon::schema_bytes_for_shape(shape).expect("phon schema bytes");
    let mut s = String::with_capacity(bytes.len() * 2);
    for b in bytes {
        s.push_str(&format!("{b:02x}"));
    }
    s
}

/// Generate the `{service}` phon registry + per-method schema table.
pub fn generate_phon_service(service: &ServiceDescriptor) -> String {
    let name = lower_camel(service.service_name);

    // One registry over every method's args + ok types (deduped, transitive).
    let mut roots: Vec<&'static Shape> = Vec::new();
    for m in service.methods {
        roots.push(m.args_shape);
        roots.push(ok_shape(m.return_shape));
    }
    let module = phon_codegen::Module::from_shapes(&roots).expect("derive service phon module");

    let mut out = String::new();
    out.push_str(
        "// phon schemas for this service (registry + per-method roots + channel metadata).\n",
    );
    out.push_str(
        "import { Registry, schemaFromBytes, hexToBytes } from \"@bearcove/phon-schema\";\n",
    );
    out.push_str("import type { Primitive } from \"@bearcove/phon-schema\";\n\n");

    out.push_str(&phon_codegen::typescript::render_registry(
        &module,
        &format!("{name}Registry"),
    ));
    out.push('\n');

    out.push_str(&format!(
        "export const {name}Methods: Record<string, import(\"@bearcove/vox-core\").PhonMethodSchemas> = {{\n"
    ));
    for m in service.methods {
        let method_id = crate::method_id(m);
        let args_root = root_id(m.args_shape);
        let ok_root = root_id(ok_shape(m.return_shape));
        let closure = args_closure_hex(m.args_shape);

        out.push_str(&format!("  \"{}\": {{\n", hex_u64(method_id)));
        out.push_str(&format!("    argsRoot: {}n,\n", hex_u64(args_root)));
        out.push_str(&format!("    argsSchemaClosure: \"{closure}\",\n"));
        out.push_str(&format!("    okRoot: {}n,\n", hex_u64(ok_root)));
        out.push_str("    channels: [");
        let mut first = true;
        for (i, arg) in m.args.iter().enumerate() {
            let dir = if is_tx(arg.shape) {
                Some("tx")
            } else if is_rx(arg.shape) {
                Some("rx")
            } else {
                None
            };
            if let Some(dir) = dir {
                let element = arg
                    .shape
                    .type_params
                    .first()
                    .map(|tp| tp.shape())
                    .unwrap_or(<() as Facet>::SHAPE);
                if !first {
                    out.push_str(", ");
                }
                first = false;
                out.push_str(&format!(
                    "{{ index: {i}, direction: \"{dir}\", elementRoot: {}n }}",
                    hex_u64(root_id(element))
                ));
            }
        }
        out.push_str("],\n");
        out.push_str("  },\n");
    }
    out.push_str("};\n");
    out
}

fn lower_camel(s: &str) -> String {
    use heck::ToLowerCamelCase;
    s.to_lower_camel_case()
}
