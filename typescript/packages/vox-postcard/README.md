# @bearcove/vox-postcard

binette compact serialization utilities for Vox TypeScript packages.

## Role in the Vox stack

This package still has its historical package name while Vox migrates its TypeScript package graph,
but the codec semantics are binette compact.

## What this package provides

- binette-oriented schema codecs
- Primitive/value encoding helpers used by protocol message serialization

## Fits with

- `@bearcove/vox-wire` for full wire message encoding
- `@bearcove/vox-core` runtime call/response machinery

Part of the Vox workspace: <https://github.com/bearcove/vox>
