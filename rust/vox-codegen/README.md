# vox-codegen

Language binding generator for Vox service descriptors.

## Role in the Vox stack

`vox-codegen` bridges Rust-defined schemas to generated clients/servers above the RPC surface.

## What this crate provides

- TypeScript code generation target
- Rendering of service descriptors into client/server scaffolding

## Fits with

- `vox` service definitions and generated descriptors
- `vox-hash` and `vox-types` for shared protocol identity and type model

Part of the Vox workspace: <https://github.com/bearcove/vox>
