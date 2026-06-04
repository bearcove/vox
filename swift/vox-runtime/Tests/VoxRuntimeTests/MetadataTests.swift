import Testing

@testable import VoxRuntime

struct MetadataTests {
    // r[verify rpc.metadata.sigils]
    @Test func metadataSigilsAreKeyStringConventions() {
        #expect(!metadataKeyIsRedacted("regular.metadata"))
        #expect(!metadataKeyIsNoPropagate("regular.metadata"))

        #expect(metadataKeyIsRedacted("#sensitive.metadata"))
        #expect(!metadataKeyIsNoPropagate("#sensitive.metadata"))

        #expect(!metadataKeyIsRedacted("-no-propagate-metadata"))
        #expect(metadataKeyIsNoPropagate("-no-propagate-metadata"))

        #expect(metadataKeyIsRedacted("-#sensitive-and-no-propagate-metadata"))
        #expect(metadataKeyIsNoPropagate("-#sensitive-and-no-propagate-metadata"))
    }
}
