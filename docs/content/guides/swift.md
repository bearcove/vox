+++
title = "Swift Status"
description = "Current status of Swift support during the binette migration."
weight = 22
+++

Swift support is parked during the binette migration.

The expected next Swift path is a Rust FFI codec boundary: binette stays in
Rust, and Swift calls into that implementation instead of maintaining a
separate Swift-native codec. The old descriptor-driven Swift codegen path is
not part of the active binette protocol direction.

Current protocol documentation covers Rust and TypeScript. Swift material will
be rewritten when the FFI-backed Swift path is active again.
