import Foundation

extension Driver {
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
            if resumable {
                pendingTaskMessages.append(DriverQueuedTaskMessage(message: message))
                _ = eventContinuation.yield(.conduitFailed(String(describing: error)))
                return
            }
            throw error
        }
    }

    private func responseMessage(
        requestId: UInt64,
        payload: [UInt8],
        schemas: [UInt8] = []
    ) async -> Message? {
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
            let sealedResponse = SealedOperationResponse(
                payload: checkedPayload,
                responseSchemaClosure: responseSchemaClosure
            )
            let waiters = await operations.seal(ownerRequestId: requestId, response: sealedResponse)
            if !waiters.isEmpty {
                for waiter in waiters {
                    guard let replay = await responseMessage(requestId: waiter, payload: checkedPayload, schemas: schemas) else {
                        continue
                    }
                    try await sendOrEnqueue(replay)
                }
                return
            }
            guard let response = await responseMessage(requestId: requestId, payload: checkedPayload, schemas: schemas) else {
                return
            }
            wireMsg = response
        }
        try await sendOrEnqueue(wireMsg)
    }

    /// Handle a command from ConnectionHandle.
    func handleCommand(_ cmd: HandleCommand) async {
        switch cmd {
        case .call(
            let requestId, let methodId, let metadata, let payload, let channels, let retry,
            let timeout, let prepareRetry, let responseTx, let schemaInfo):
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
                retry: retry,
                timeout: timeout,
                prepareRetry: prepareRetry,
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
                if resumable {
                    pendingCalls.append(queuedCall)
                    _ = eventContinuation.yield(.conduitFailed(String(describing: error)))
                    return
                }
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

        traceLog(.resume, "flushPendingCalls: count=\(pendingCalls.count)")
        while let call = pendingCalls.first {
            let replayCall: DriverQueuedCall
            if let prepareRetry = call.prepareRetry {
                traceLog(.resume, "flushPendingCalls: rebuilding requestId=\(call.requestId) methodId=\(call.methodId)")
                let rebuilt = await prepareRetry()
                let replayMetadata =
                    if call.retry.idem {
                        await handle.freshOperationMetadata(from: call.metadata)
                    } else {
                        call.metadata
                    }
                replayCall = DriverQueuedCall(
                    requestId: call.requestId,
                    methodId: call.methodId,
                    metadata: replayMetadata,
                    payload: rebuilt.payload,
                    channels: rebuilt.channels,
                    retry: call.retry,
                    timeout: call.timeout,
                    prepareRetry: call.prepareRetry,
                    schemaInfo: call.schemaInfo
                )
            } else {
                replayCall = call
            }

            // Advertise the args schema closure (on replay, may already be sent → deduped).
            let schemas: [UInt8]
            if let schemaInfo = replayCall.schemaInfo {
                schemas = schemaSendTracker.prepareSchemas(
                    replayCall.methodId, .args, schemaInfo.methodSchemas.argsSchemaClosure)
            } else {
                schemas = []
            }

            let msg = messageRequest(
                requestId: replayCall.requestId,
                methodId: replayCall.methodId,
                payload: replayCall.payload,
                metadata: replayCall.metadata,
                channels: replayCall.channels,
                schemas: schemas
            )

            traceLog(.resume, "flushPendingCalls: sending replay requestId=\(replayCall.requestId) methodId=\(replayCall.methodId)")
            do {
                try await conduit.send(msg)
            } catch TransportError.wouldBlock {
                traceLog(.resume, "flushPendingCalls: conduit would block requestId=\(replayCall.requestId)")
                pendingCalls[0] = replayCall
                return
            } catch {
                if resumable {
                    traceLog(.resume, "flushPendingCalls: send failed requestId=\(replayCall.requestId) error=\(String(describing: error))")
                    pendingCalls[0] = replayCall
                    _ = eventContinuation.yield(.conduitFailed(String(describing: error)))
                    return
                }
                let pending = await state.claimPendingResponse(
                    replayCall.requestId,
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

            guard let timeout = replayCall.timeout else {
                continue
            }

            let timeoutNs = Self.timeoutToNanoseconds(timeout)
            let capturedState = state
            let capturedConduit = conduit
            let requestId = replayCall.requestId
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

    func replayPendingCallsAfterResume() async {
        let inFlight = await state.pendingCallsSnapshot()
        pendingCalls.removeAll()
        traceLog(.resume, "replayPendingCallsAfterResume: inFlight=\(inFlight.count)")
        for call in inFlight {
            if call.prepareRetry != nil && !call.retry.idem {
                traceLog(.resume, "replayPendingCallsAfterResume: indeterminate requestId=\(call.requestId)")
                guard let pending = await state.claimPendingResponse(
                    call.requestId,
                    reason: "resume-channel-indeterminate"
                ) else {
                    continue
                }
                pending.timeoutTask?.cancel()
                // Deliver a typed Result.Err(VoxError.Indeterminate): the dispatcher
                // encodes it through a response program (Err is T/E-independent on the
                // wire), and the generated client decodes it back to .indeterminate.
                pending.responseTx(.success(dispatcher.encodeVoxError(.indeterminate)))
                continue
            }
            traceLog(.resume, "replayPendingCallsAfterResume: queueing requestId=\(call.requestId) methodId=\(call.methodId)")
            pendingCalls.append(call)
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
                if resumable {
                    _ = eventContinuation.yield(.conduitFailed(String(describing: error)))
                    return
                }
                await failAllPending()
                eventContinuation.finish()
                return
            }

            pendingTaskMessages.removeFirst()
        }
    }
}
