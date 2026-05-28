+++
title = "Swift Status"
description = "Current status of Swift support through binette local access."
weight = 22
+++

Swift support is being rebuilt around binette local access.

The wire contract is binette, not Swift and not Rust. Swift describes local
values by handing binette a tree of C ABI descriptors: layout facts, field
offsets, enum projection/constructor thunks, sequence thunks, option layout,
and external attachment markers such as Vox channels. The shared binette
runtime then uses the same schema/value machinery as Rust.

The current Swift work in this repository is a canary for that boundary. It
depends on the local binette checkout's Swift probes and verifies that
Vox-shaped Swift values cross the descriptor import surface, generate binette
schema bundles, convert those bundles into Vox schema payload bytes, encode and
decode through binette, and translate between distinct writer/reader schema
bundles. The Rust receive path accepts Swift-derived schema payload bytes
through `SchemaRecvTracker` and consumes Swift-encoded argument bytes through
the normal Vox argument deserializer. The bridge also converts Rust-produced Vox
response schema payloads back into binette schema bundles so Swift can decode
Rust response bytes into Swift local values. Full Swift RPC client/server
support will build on this path rather than reintroducing a separate
Swift-native codec.
