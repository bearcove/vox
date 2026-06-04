import Foundation

extension Driver {
    // r[impl rpc.observability.runtime]
    // r[impl rpc.observability.driver]
    func drainInjectedQueues() async throws {
        let commands = commandQueue.popAll()
        for command in commands {
            await handleCommand(command)
        }
        let taskMessages = taskQueue.popAll()
        for message in taskMessages {
            try await handleTaskMessage(message)
        }
    }

    /// Spawn a reader task that reads from the conduit and yields events.
    /// r[impl rpc.observability.session-errors]
    private func spawnReaderTask(
        for conduit: any Conduit,
        continuation: AsyncStream<DriverEvent>.Continuation
    ) -> Task<Void, Never> {
        Task {
            do {
                while !Task.isCancelled {
                    if let msg = try await conduit.recv() {
                        traceLog(.driver, "reader received message")
                        continuation.yield(.incomingMessage(msg))
                    } else {
                        traceLog(.driver, "reader observed conduit close")
                        continuation.yield(.conduitClosed)
                        break
                    }
                }
            } catch {
                if !Task.isCancelled {
                    traceLog(.driver, "reader failed: \(String(describing: error))")
                    continuation.yield(.conduitFailed(String(describing: error)))
                }
            }
        }
    }

    /// Run the driver until connection closes.
    /// r[impl rpc]
    /// r[impl rpc.service]
    /// r[impl rpc.handler]
    /// r[impl rpc.pipelining]
    /// r[impl rpc.observability.runtime]
    /// r[impl rpc.observability.driver]
    /// r[impl rpc.observability.session-errors]
    public func run() async throws {
        var keepaliveRuntime = makeKeepaliveRuntime()
        traceLog(.driver, "run start")

        let cont = eventContinuation
        let readerTask = spawnReaderTask(for: conduit, continuation: cont)

        let keepaliveTask = Task {
            while !Task.isCancelled {
                try? await Task.sleep(nanoseconds: 10_000_000)
                cont.yield(.keepaliveTick)
            }
        }

        defer {
            readerTask.cancel()
            keepaliveTask.cancel()
            commandQueue.close()
            taskQueue.close()
            eventContinuation.finish()
        }

        do {
            for await event in eventStream {
                try await drainInjectedQueues()
                try await flushPendingTaskMessages()
                try await flushPendingCalls()

                switch event {
                case .incomingMessage(let msg):
                    try await handleMessage(msg, keepaliveRuntime: &keepaliveRuntime)

                case .wake:
                    break

                case .keepaliveTick:
                    try await handleKeepaliveTick(keepaliveRuntime: &keepaliveRuntime)

                case .conduitClosed, .conduitFailed:
                    traceLog(.driver, "conduit broke")
                    await failAllPending()
                    eventContinuation.finish()
                }
            }
        } catch {
            traceLog(.driver, "run threw: \(String(describing: error))")
            eventContinuation.finish()
            await failAllPending()
            try? await conduit.close()
            throw error
        }
        traceLog(.driver, "run exiting")
        await failAllPending()
        try? await conduit.close()
    }
}
