+++
title = "Swift Status"
description = "Current status of Swift support through binette local access."
weight = 22
+++

Swift support is built around binette local access.

The wire contract is binette, not Swift and not Rust. Swift describes local
values by handing binette a tree of C ABI descriptors: layout facts, field
offsets, enum projection/constructor thunks, sequence thunks, option layout,
and external attachment markers such as Vox channels. The shared binette
runtime then uses the same schema/value machinery as Rust.

The current Swift work depends on the local binette checkout's Swift probes and
verifies that Vox-shaped Swift values cross the descriptor import surface,
generate binette schema bundles, convert those bundles into Vox schema payload
bytes, encode and decode through binette, and translate between distinct
writer/reader schema bundles. The Rust receive path accepts Swift-derived
schema payload bytes through `SchemaRecvTracker` and consumes Swift-encoded
argument bytes through the normal Vox argument deserializer. The bridge also
converts Rust-produced Vox response schema payloads back into binette schema
bundles so Swift can decode Rust response bytes into Swift local values.

`VoxSwiftMethodCodec` packages that boundary for generated Swift stubs: encode
local args into a Vox wire payload and decode a response wire payload into the
local response type. `vox-codegen` emits Swift value declarations,
method-argument tuple carriers, binette C ABI descriptor functions, and
method-codec accessors for the supported shape set. Unsupported Swift local
layouts are rejected at generation time instead of falling back to a fake codec
path.

The driver-backed canary sends Swift-produced argument schema and payload bytes
through the Rust Vox driver over an in-memory link and decodes the Rust response
back into a Swift value. It covers a Vox-shaped Swift struct with string, bytes,
option, enum payload thunking, and external channel metadata. Full Swift RPC
client/server support stays on this path rather than introducing a separate
Swift-native codec.
