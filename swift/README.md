# Swift Status

Swift support is active again through the `VoxSwift` package and binette local
access.

The Swift side does not define a separate codec. Swift code describes local
values with binette C ABI descriptors and thunks, and binette remains the
shared schema/value/wire layer.

The canaries in this directory are intentionally small but real. They exercise
Vox-shaped Swift values through binette C ABI descriptor import, schema bundle
generation, encode/decode, writer/reader schema translation, and the Vox driver
boundary over an in-memory link. Full Swift RPC runtime support is built on top
of this path.
