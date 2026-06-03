import Foundation

extension Driver {
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
    public func run() async throws {
        var keepaliveRuntime = makeKeepaliveRuntime()
        traceLog(.driver, "run start")

        let cont = eventContinuation
        let readerTask = spawnReaderTask(for: conduit, continuation: cont)

        let retryTask = Task {
            while !Task.isCancelled {
                try? await Task.sleep(nanoseconds: 10_000_000)
                cont.yield(.retryTick)
            }
        }

        defer {
            readerTask.cancel()
            retryTask.cancel()
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

                case .retryTick:
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
