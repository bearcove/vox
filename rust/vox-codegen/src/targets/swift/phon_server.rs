//! Swift phon server emitter: the `{Service}Handler` protocol (the user implements
//! it) + a `{Service}Dispatcher: ServiceDispatcher` that decodes args via the typed
//! path (reconciling the peer's writer schema), calls the handler, wraps the result
//! into `Result<T, VoxError<E>>`, encodes + advertises it, and replies. Replaces the
//! postcard `server.rs`.

use heck::{ToLowerCamelCase, ToUpperCamelCase};
use vox_types::{MethodDescriptor, ServiceDescriptor, ShapeKind, classify_shape, is_rx, is_tx};

use super::phon_service::method_global_prefix;
use super::types::{format_doc, swift_type_base, swift_type_server_arg, swift_type_server_return};
use crate::render::hex_u64;

fn has_channels(method: &MethodDescriptor) -> bool {
    method.args.iter().any(|a| is_tx(a.shape) || is_rx(a.shape))
}

/// `(ret_ty, response_wire_ty, user_error_ty?)`. `user_error_ty` is `Some` only for a
/// fallible method (`Result<T, E>` return) — its `E` is the handler's thrown error.
fn method_types(method: &MethodDescriptor) -> (String, String, Option<String>) {
    let ret = swift_type_server_return(method.return_shape);
    let resp = swift_type_base(method.response_wire_shape);
    let user_err = match classify_shape(method.return_shape) {
        ShapeKind::Result { err, .. } => Some(swift_type_base(err)),
        _ => None,
    };
    (ret, resp, user_err)
}

pub fn generate_phon_server(service: &ServiceDescriptor) -> String {
    let service_name = service.service_name.to_upper_camel_case();
    let mut out = String::new();

    // Handler protocol (implemented by the user).
    if let Some(doc) = &service.doc {
        out.push_str(&format_doc(doc, ""));
    }
    out.push_str(&format!(
        "public protocol {service_name}Handler: Sendable {{\n"
    ));
    for method in service.methods {
        if let Some(doc) = &method.doc {
            out.push_str(&format_doc(doc, "    "));
        }
        let name = method.method_name.to_lower_camel_case();
        let args: Vec<String> = method
            .args
            .iter()
            .map(|a| {
                format!(
                    "{}: {}",
                    a.name.to_lower_camel_case(),
                    swift_type_server_arg(a.shape)
                )
            })
            .collect();
        let ret = swift_type_server_return(method.return_shape);
        if ret == "Void" {
            out.push_str(&format!(
                "    func {name}({}) async throws\n",
                args.join(", ")
            ));
        } else {
            out.push_str(&format!(
                "    func {name}({}) async throws -> {ret}\n",
                args.join(", ")
            ));
        }
    }
    out.push_str("}\n\n");

    // Dispatcher.
    out.push_str(&format!(
        "public final class {service_name}Dispatcher: ServiceDispatcher {{\n"
    ));
    out.push_str(&format!("    private let handler: {service_name}Handler\n"));
    out.push_str(&format!(
        "    public init(handler: {service_name}Handler) {{ self.handler = handler }}\n\n"
    ));

    // retryPolicy
    out.push_str("    public func retryPolicy(methodId: UInt64) -> RetryPolicy {\n        switch methodId {\n");
    for m in service.methods {
        out.push_str(&format!(
            "        case {}: return RetryPolicy(persist: {}, idem: {})\n",
            hex_u64(crate::method_id(m)),
            m.retry_persist,
            m.retry_idem
        ));
    }
    out.push_str("        default: return .volatile\n        }\n    }\n\n");

    // encodeVoxError — encode a runtime error through any method's response type
    // (the non-User Err arms are independent of `E`). Use the first method.
    let m0 = &service.methods[0];
    let prefix0 = method_global_prefix(service.service_name, m0.method_name);
    let resp0 = swift_type_base(m0.response_wire_shape);
    let wire0 = match classify_shape(m0.response_wire_shape) {
        ShapeKind::Result { err, .. } => swift_type_base(err),
        _ => "VoxError<Infallible>".to_string(),
    };
    out.push_str(&format!(
        "    public func encodeVoxError(_ error: VoxRuntimeError) -> [UInt8] {{\n        let wire: {wire0}\n        switch error {{\n        case .unknownMethod, .notImplemented: wire = .unknownMethod\n        case .invalidPayload(let s), .decodeError(let s), .encodeError(let s): wire = .invalidPayload(s)\n        case .cancelled: wire = .cancelled\n        case .connectionClosed: wire = .connectionClosed\n        case .timeout, .indeterminate: wire = .indeterminate\n        }}\n        var r: {resp0} = .failure(wire)\n        return withUnsafeBytes(of: &r) {{ encodeWith({prefix0}_ResponseEncodeProgram, $0.baseAddress!) }}\n    }}\n\n"
    ));

    // preregister — channels (out-of-band binding) deferred.
    out.push_str("    public func preregister(methodId: UInt64, payload: [UInt8], registry: ChannelRegistry) async {}\n\n");

    // dispatch — route to per-method helpers.
    out.push_str("    public func dispatch(methodId: UInt64, payload: [UInt8], requestId: UInt64, registry: ChannelRegistry, schemaSendTracker: SchemaSendTracker, schemaReceiveTracker: SchemaTracker, taskTx: @escaping @Sendable (TaskMessage) -> Void) async {\n        switch methodId {\n");
    for m in service.methods {
        let id = hex_u64(crate::method_id(m));
        let name = m.method_name.to_lower_camel_case();
        out.push_str(&format!(
            "        case {id}: await dispatch_{name}(payload: payload, requestId: requestId, schemaSendTracker: schemaSendTracker, schemaReceiveTracker: schemaReceiveTracker, taskTx: taskTx)\n"
        ));
    }
    out.push_str("        default: taskTx(.response(requestId: requestId, payload: encodeVoxError(.unknownMethod), methodId: methodId))\n        }\n    }\n\n");

    // Per-method dispatch helpers.
    for m in service.methods {
        out.push_str(&generate_dispatch_method(service, m));
    }

    out.push_str("}\n\n");
    out
}

fn generate_dispatch_method(service: &ServiceDescriptor, m: &MethodDescriptor) -> String {
    let id = hex_u64(crate::method_id(m));
    let name = m.method_name.to_lower_camel_case();
    let prefix = method_global_prefix(service.service_name, m.method_name);
    let svc = service.service_name.to_lower_camel_case();
    let (ret_ty, resp_ty, user_err) = method_types(m);
    let args_ty = swift_type_base(m.args_shape);
    let arity = m.args.len();
    let mut out = String::new();

    out.push_str(&format!(
        "    private func dispatch_{name}(payload: [UInt8], requestId: UInt64, schemaSendTracker: SchemaSendTracker, schemaReceiveTracker: SchemaTracker, taskTx: @escaping @Sendable (TaskMessage) -> Void) async {{\n"
    ));

    if has_channels(m) {
        out.push_str(&format!(
            "        // Channel method — out-of-band binding not yet wired in the Swift phon server.\n        taskTx(.response(requestId: requestId, payload: encodeVoxError(.invalidPayload(\"channel method `{name}` not wired\")), methodId: {id}))\n    }}\n\n"
        ));
        return out;
    }

    // Decode args (reconciling the peer's writer schema when advertised).
    if arity == 0 {
        // No args to decode.
    } else {
        out.push_str(&format!(
            "        let argsProgram = schemaReceiveTracker.buildDecodeProgram({id}, .args, readerDescriptor: {prefix}_ArgsDescriptor, local: {svc}Registry) ?? {prefix}_ArgsDecodeProgram\n"
        ));
        out.push_str(&format!(
            "        let argsRaw = UnsafeMutableRawPointer.allocate(byteCount: MemoryLayout<{args_ty}>.size, alignment: MemoryLayout<{args_ty}>.alignment)\n        defer {{ argsRaw.deallocate() }}\n"
        ));
        out.push_str(&format!(
            "        do {{ try decodeInto(argsProgram, payload, argsRaw) }} catch {{\n            taskTx(.response(requestId: requestId, payload: encodeVoxError(.invalidPayload(\"decode args\")), methodId: {id}))\n            return\n        }}\n        let args = argsRaw.assumingMemoryBound(to: {args_ty}.self).move()\n"
        ));
    }

    // The handler call expression, with labels.
    let call_args: Vec<String> = m
        .args
        .iter()
        .enumerate()
        .map(|(i, a)| {
            let label = a.name.to_lower_camel_case();
            let value = if arity == 1 {
                "args".to_string()
            } else {
                format!("args.{i}")
            };
            format!("{label}: {value}")
        })
        .collect();
    let call = format!("handler.{name}({})", call_args.join(", "));

    // Call + wrap into the wire `Result<T, VoxError<E>>`. A fallible handler returns
    // `Result<T, E>` (its `.failure(e)` becomes the wire `User(e)`); an infallible one
    // returns `T`/`Void`. An unexpected throw maps to `Indeterminate`.
    out.push_str(&format!("        let result: {resp_ty}\n        do {{\n"));
    if user_err.is_some() {
        out.push_str(&format!("            let userResult = try await {call}\n"));
        out.push_str(
            "            switch userResult {\n            case .success(let v): result = .success(v)\n            case .failure(let e): result = .failure(.user(e))\n            }\n",
        );
    } else if ret_ty == "Void" {
        out.push_str(&format!(
            "            try await {call}\n            result = .success(())\n"
        ));
    } else {
        out.push_str(&format!(
            "            let value = try await {call}\n            result = .success(value)\n"
        ));
    }
    out.push_str("        } catch {\n            result = .failure(.indeterminate)\n        }\n");

    // Encode the response, advertise its schema (once), and reply.
    out.push_str(&format!(
        "        var r = result\n        let respPayload = withUnsafeBytes(of: &r) {{ encodeWith({prefix}_ResponseEncodeProgram, $0.baseAddress!) }}\n        let schemas = schemaSendTracker.prepareSchemas({id}, .response, {svc}Methods[{id}]!.responseSchemaClosure)\n        taskTx(.response(requestId: requestId, payload: respPayload, methodId: {id}, schemas: schemas))\n    }}\n\n"
    ));

    out
}
