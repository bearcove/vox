//! Swift phon client emitter: the `{Service}Caller` protocol + `{Service}Client`
//! whose method bodies encode args via the typed path, call the runtime with the
//! method's `ClientSchemaInfo`, decode the `Result<T, VoxError<E>>` response, and
//! unwrap (throw on `Err`). Replaces the postcard `client.rs` bodies.

use heck::{ToLowerCamelCase, ToUpperCamelCase};
use vox_types::{ServiceDescriptor, is_rx, is_tx};

use super::phon_service::method_global_prefix;
use super::types::{format_doc, swift_type_base, swift_type_client_arg, swift_type_client_return};
use crate::render::hex_u64;

/// A method's signature `name(arg: T, …)` and its return type (or `Void`).
fn method_signature(method: &vox_types::MethodDescriptor) -> (String, String, String) {
    let name = method.method_name.to_lower_camel_case();
    let args: Vec<String> = method
        .args
        .iter()
        .map(|a| {
            format!(
                "{}: {}",
                a.name.to_lower_camel_case(),
                swift_type_client_arg(a.shape)
            )
        })
        .collect();
    let ret = swift_type_client_return(method.return_shape);
    (name, args.join(", "), ret)
}

pub fn generate_phon_client(service: &ServiceDescriptor) -> String {
    let service_name = service.service_name.to_upper_camel_case();
    let svc = service.service_name.to_lower_camel_case();
    let mut out = String::new();

    // The caller protocol.
    if let Some(doc) = &service.doc {
        out.push_str(&format_doc(doc, ""));
    }
    out.push_str(&format!(
        "public protocol {service_name}Caller: Sendable {{\n"
    ));
    for method in service.methods {
        if let Some(doc) = &method.doc {
            out.push_str(&format_doc(doc, "    "));
        }
        let (name, args, ret) = method_signature(method);
        if ret == "Void" {
            out.push_str(&format!("    func {name}({args}) async throws\n"));
        } else {
            out.push_str(&format!("    func {name}({args}) async throws -> {ret}\n"));
        }
    }
    out.push_str("}\n\n");

    // The client.
    out.push_str(&format!(
        "public final class {service_name}Client: {service_name}Caller, Sendable {{\n"
    ));
    out.push_str("    private let connection: VoxConnection\n");
    out.push_str("    private let timeout: TimeInterval?\n\n");
    out.push_str(
        "    public init(connection: VoxConnection, timeout: TimeInterval? = 30.0) {\n        self.connection = connection\n        self.timeout = timeout\n    }\n\n",
    );

    for method in service.methods {
        let (name, args, ret) = method_signature(method);
        let method_id = hex_u64(crate::method_id(method));
        let prefix = method_global_prefix(service.service_name, method.method_name);
        let resp_ty = swift_type_base(method.response_wire_shape);
        let has_channels = method.args.iter().any(|a| is_tx(a.shape) || is_rx(a.shape));

        let sig = if ret == "Void" {
            format!("    public func {name}({args}) async throws {{\n")
        } else {
            format!("    public func {name}({args}) async throws -> {ret} {{\n")
        };
        out.push_str(&sig);

        if has_channels {
            // Channels ride out-of-band (PhonChannelMeta); the client must allocate
            // ids, replace the Tx/Rx args with their wire index, and pass channel ids
            // in the call. Not wired yet — fail loudly (echo + non-channel methods work).
            out.push_str(&format!(
                "        fatalError(\"phon Swift client: channel method `{name}` not yet wired\")\n    }}\n\n"
            ));
            continue;
        }

        // Encode args via the typed path. 0 args → empty; 1 arg → the bare value;
        // N args → a Swift tuple (the descriptor is a positional record over it).
        let arg_names: Vec<String> = method
            .args
            .iter()
            .map(|a| a.name.to_lower_camel_case())
            .collect();
        match arg_names.len() {
            0 => out.push_str("        let payload: [UInt8] = []\n"),
            1 => {
                out.push_str(&format!("        var argsValue = {}\n", arg_names[0]));
                out.push_str(&format!(
                    "        let payload = withUnsafeBytes(of: &argsValue) {{ encodeWith({prefix}_ArgsEncodeProgram, $0.baseAddress!) }}\n"
                ));
            }
            _ => {
                out.push_str(&format!(
                    "        var argsValue = ({})\n",
                    arg_names.join(", ")
                ));
                out.push_str(&format!(
                    "        let payload = withUnsafeBytes(of: &argsValue) {{ encodeWith({prefix}_ArgsEncodeProgram, $0.baseAddress!) }}\n"
                ));
            }
        }

        // Call the runtime with this method's schema info (advertises args closure).
        out.push_str(&format!(
            "        let response = try await connection.call(\n            methodId: {method_id}, metadata: .null, payload: payload, retry: .volatile,\n            timeout: timeout, prepareRetry: nil, finalizeChannels: nil,\n            schemaInfo: ClientSchemaInfo(methodSchemas: {svc}Methods[{method_id}]!, registry: {svc}Registry))\n"
        ));

        // Decode the Result<T, VoxError<E>> response and unwrap.
        out.push_str(&format!(
            "        let raw = UnsafeMutableRawPointer.allocate(byteCount: MemoryLayout<{resp_ty}>.size, alignment: MemoryLayout<{resp_ty}>.alignment)\n        defer {{ raw.deallocate() }}\n        try decodeInto({prefix}_ResponseDecodeProgram, response, raw)\n        let result = raw.assumingMemoryBound(to: {resp_ty}.self).move()\n        switch result {{\n"
        ));
        if ret == "Void" {
            out.push_str("        case .success: return\n");
        } else {
            out.push_str("        case .success(let value): return value\n");
        }
        out.push_str("        case .failure(let error): throw error\n        }\n    }\n\n");
    }

    out.push_str("}\n\n");
    out
}
