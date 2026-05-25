+++
title = "Specification"
description = "Formal specification for the Vox protocol, transports, and stability rules."
weight = 10
+++

The Vox specification defines the protocol and runtime model across layers. Vox
uses [Binette](https://github.com/bearcove/binette) as its value format, but
Binette is specified separately.

- Requests and channels
- Connections and sessions
- Transport prologue and conduit selection
- Conduit behavior
- Retry semantics and operation continuity
- Link transports (stream and WebSocket)

Start with [Introduction](./intro/), then continue through the protocol chapters in this section.
