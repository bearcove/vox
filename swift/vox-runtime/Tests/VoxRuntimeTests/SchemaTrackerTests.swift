import Testing

@testable import VoxRuntime

@Test
// r[verify schema.format.delivery]
func schemaSendTrackerAdvertisesBindingOncePerDirection() {
    let tracker = SchemaSendTracker()
    let closure: [UInt8] = [1, 2, 3]

    #expect(tracker.prepareSchemas(7, .args, closure) == closure)
    #expect(tracker.prepareSchemas(7, .args, closure).isEmpty)
    #expect(tracker.prepareSchemas(7, .response, closure) == closure)
}
