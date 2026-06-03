import Foundation

public final class SessionHandle: @unchecked Sendable {
    private let eventContinuation: AsyncStream<DriverEvent>.Continuation

    init(
        eventContinuation: AsyncStream<DriverEvent>.Continuation
    ) {
        self.eventContinuation = eventContinuation
    }

    /// Shutdown the session.
    public func shutdown() {
        eventContinuation.finish()
    }
}
