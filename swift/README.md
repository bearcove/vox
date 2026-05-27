# Swift Status

Swift support is active again through binette local access.

The Swift side does not define a separate codec. Swift code describes local
values with binette C ABI descriptors and thunks, and binette remains the
shared schema/value/wire layer.

The canaries in this directory are intentionally small: they keep the Vox side
honest that Swift is in bounds while the full Swift RPC runtime is rebuilt on
top of binette.
