# Swift Status

Swift support is parked during the binette migration.

The previous native Swift runtime and subject carried the retired postcard/CBOR
protocol model, so they have been removed from the active tree. The expected
next Swift path is a Rust FFI boundary that calls the Rust binette
implementation instead of maintaining a separate Swift-native codec.
