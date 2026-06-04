import Foundation

extension Driver {
    // r[impl rpc.flow-control]
    private func sendOrEnqueue(_ message: Message) async throws {
        if !pendingTaskMessages.isEmpty {
            pendingTaskMessages.append(DriverQueuedTaskMessage(message: message))
            return
        }

        do {
            try await conduit.send(message)
        } catch TransportError.wouldBlock {
            pendingTaskMessages.append(DriverQueuedTaskMessage(message: message))
        } catch {
            throw error
        }
    }

    private func responseMessage(
        requestId: UInt64,
        payload: [UInt8],
        schemas: [UInt8] = []
    ) async -> Message? {
        // r[impl rpc.response]
        // r[impl rpc.response.one-per-request]
        let responseContext = await state.removeInFlight(requestId)
        guard responseContext.removed else {
            return nil
        }
        return messageResponse(
            requestId: requestId,
            payload: payload,
            metadata: responseContext.responseMetadata,
            connectionId: responseContext.connectionId,
            schemas: schemas
        )
    }

    /// Get the task sender for handlers to send responses.
    func taskSender() -> @Sendable (TaskMessage) -> Void {
        let cont = eventContinuation
        let queue = taskQueue
        return { msg in
            guard queue.push(msg) else {
                return
            }
            _ = cont.yield(.wake)
        }
    }

    /// Handle a task message from a handler.
    /// r[impl rpc.response]
    /// r[impl rpc.channel.connection-closure]
    func handleTaskMessage(_ msg: TaskMessage) async throws {
        let wireMsg: Message
        switch msg {
        case .data(let channelId, let payload):
            wireMsg = messageData(channelId: channelId, item: payload)
        case .close(let channelId):
            wireMsg = messageChannelClose(channelId: channelId)
        case .grantCredit(let channelId, let bytes):
            wireMsg = messageCredit(channelId: channelId, additional: bytes)
        case .schema(let methodId, let direction, let schemas):
            wireMsg = messageSchema(methodId: methodId, direction: direction, schemas: schemas)
        case .response(let requestId, let payload, let methodId, let responseSchemaClosure):
            // Advertise the response schema at THIS sequential send point (not in the
            // concurrent dispatch task): under pipelining many responses for a method
            // are written here in order, and the first one MUST carry the schema. A
            // dispatch-time decision races — a schema-less response could be written
            // first. prepareSchemas is idempotent, so only the first send advertises.
            // r[impl schema.exchange.required]
            // r[impl schema.exchange.callee]
            let schemas: [UInt8]
            if let methodId, !responseSchemaClosure.isEmpty {
                schemas = schemaSendTracker.prepareSchemas(
                    methodId, .response, responseSchemaClosure)
            } else {
                schemas = []
            }
            debugLog(
                "send Response req=\(requestId) payloadLen=\(payload.count) "
                    + "schemasLen=\(schemas.count)")
            let checkedPayload: [UInt8]
            if payload.count > Int(negotiated.maxPayloadSize) {
                debugLog(
                    "outgoing response for request \(requestId) exceeds max_payload_size "
                        + "(\(payload.count) > \(negotiated.maxPayloadSize)), sending Cancelled")
                // Replace the over-sized payload with a typed `Cancelled` VoxError (its
                // Err arm is T-independent on the wire, so any method's response program
                // encodes it).
                checkedPayload = dispatcher.encodeVoxError(.cancelled)
            } else {
                checkedPayload = payload
            }
            guard let response = await responseMessage(requestId: requestId, payload: checkedPayload, schemas: schemas) else {
                return
            }
            wireMsg = response
        }
        try await sendOrEnqueue(wireMsg)
    }

    /// Handle a command from ConnectionHandle.
    /// r[impl rpc.caller]
    /// r[impl rpc.request]
    /// r[impl rpc.pipelining]
    func handleCommand(_ cmd: HandleCommand) async {
        switch cmd {
        case .call(
            let requestId, let methodId, let metadata, let payload, let channels,
            let timeout, let responseTx, let schemaInfo):
            let isClosed = await state.isConnectionClosed()
            guard !isClosed else {
                responseTx(.failure(.connectionClosed))
                return
            }

            let queuedCall = DriverQueuedCall(
                requestId: requestId,
                methodId: methodId,
                metadata: metadata,
                payload: payload,
                channels: channels,
                timeout: timeout,
                schemaInfo: schemaInfo
            )

            let inserted = await state.addPendingResponse(
                requestId,
                request: queuedCall,
                responseTx,
                timeoutTask: nil
            )
            guard inserted else {
                responseTx(.failure(.connectionClosed))
                return
            }

            // Advertise the args schema closure (at most once per method, deduped).
            // r[impl schema.exchange.caller]
            let schemas: [UInt8]
            if let schemaInfo {
                schemas = schemaSendTracker.prepareSchemas(
                    methodId, .args, schemaInfo.methodSchemas.argsSchemaClosure)
            } else {
                schemas = []
            }

            let msg = messageRequest(
                requestId: requestId,
                methodId: methodId,
                payload: payload,
                metadata: metadata,
                channels: channels,
                schemas: schemas
            )
            do {
                try await conduit.send(msg)
            } catch TransportError.wouldBlock {
                pendingCalls.append(queuedCall)
                return
            } catch {
                let pending = await state.claimPendingResponse(
                    requestId,
                    reason: "conduit-send-failed"
                )
                pending?.timeoutTask?.cancel()
                warnLog("conduit send failed for request_id \(requestId): \(String(describing: error))")
                pending?.responseTx(.failure(.transportError(String(describing: error))))
                await failAllPending()
                eventContinuation.finish()
                return
            }

            guard let timeout else {
                return
            }

            let timeoutNs = Self.timeoutToNanoseconds(timeout)
            let capturedState = state
            let capturedConduit = conduit
            let timeoutTask = Task {
                do {
                    try await Task.sleep(nanoseconds: timeoutNs)
                } catch {
                    return
                }
                guard let pending = await capturedState.claimPendingResponse(
                    requestId,
                    reason: "timeout"
                ) else {
                    return
                }
                pending.timeoutTask?.cancel()
                warnLog("request timed out request_id=\(requestId) timeout_s=\(timeout)")
                pending.responseTx(.failure(.timeout))
                try? await capturedConduit.send(messageCancel(requestId: requestId))
            }
            let installed = await state.setPendingTimeoutTask(requestId, timeoutTask: timeoutTask)
            if !installed {
                timeoutTask.cancel()
            }
        }
    }

    func flushPendingCalls() async throws {
        if pendingCalls.isEmpty {
            return
        }

        while let call = pendingCalls.first {
            // Advertise the args schema closure (at most once per method, deduped).
            // r[impl schema.exchange.caller]
            let schemas: [UInt8]
            if let schemaInfo = call.schemaInfo {
                schemas = schemaSendTracker.prepareSchemas(
                    call.methodId, .args, schemaInfo.methodSchemas.argsSchemaClosure)
            } else {
                schemas = []
            }

            let msg = messageRequest(
                requestId: call.requestId,
                methodId: call.methodId,
                payload: call.payload,
                metadata: call.metadata,
                channels: call.channels,
                schemas: schemas
            )

            do {
                try await conduit.send(msg)
            } catch TransportError.wouldBlock {
                return
            } catch {
                let pending = await state.claimPendingResponse(
                    call.requestId,
                    reason: "conduit-send-failed"
                )
                pending?.timeoutTask?.cancel()
                pending?.responseTx(.failure(.transportError(String(describing: error))))
                pendingCalls.removeFirst()
                await failAllPending()
                eventContinuation.finish()
                return
            }

            pendingCalls.removeFirst()

            guard let timeout = call.timeout else {
                continue
            }

            let timeoutNs = Self.timeoutToNanoseconds(timeout)
            let capturedState = state
            let capturedConduit = conduit
            let requestId = call.requestId
            let timeoutTask = Task {
                do {
                    try await Task.sleep(nanoseconds: timeoutNs)
                } catch {
                    return
                }
                guard let pending = await capturedState.claimPendingResponse(
                    requestId,
                    reason: "timeout"
                ) else {
                    return
                }
                pending.timeoutTask?.cancel()
                warnLog("request timed out request_id=\(requestId) timeout_s=\(timeout)")
                pending.responseTx(.failure(.timeout))
                try? await capturedConduit.send(messageCancel(requestId: requestId))
            }
            let installed = await state.setPendingTimeoutTask(requestId, timeoutTask: timeoutTask)
            if !installed {
                timeoutTask.cancel()
            }
        }
    }

    func flushPendingTaskMessages() async throws {
        if pendingTaskMessages.isEmpty {
            return
        }

        while let pending = pendingTaskMessages.first {
            do {
                try await conduit.send(pending.message)
            } catch TransportError.wouldBlock {
                return
            } catch {
                await failAllPending()
                eventContinuation.finish()
                return
            }

            pendingTaskMessages.removeFirst()
        }
    }
}
