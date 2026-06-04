actor RequestIdAllocator {
    private var nextId: UInt64 = 1

    // r[impl rpc.request.id-allocation]
    func allocate() -> UInt64 {
        let id = nextId
        nextId += 1
        return id
    }
}
