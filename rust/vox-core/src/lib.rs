//! Core implementations for the vox connectivity layer.
//!
//! This crate provides concrete implementations of the traits defined in
//! [`vox_types`]. The only conduit shape is [`BareConduit`]: wraps a raw
//! `Link` with phon serialization. No reconnect, no reliability —
//! reconnect was removed (StableConduit deleted) because the abstraction
//! had no real users.

mod bare_conduit;
pub use bare_conduit::*;
pub use vox_types::TransportMode;

mod handshake;
pub use handshake::*;

mod into_conduit;
pub use into_conduit::*;

mod transport_prologue;
pub use transport_prologue::*;

mod link_source;
pub use link_source::*;

#[cfg(not(target_arch = "wasm32"))]
mod memory_link;
#[cfg(not(target_arch = "wasm32"))]
pub use memory_link::*;

mod session;
pub use session::*;

mod driver;
pub use driver::*;

/// The peer's `Message` envelope schema, carried from the handshake into the
/// conduit's Rx half.
///
/// The envelope is an evolvable wire type like any other: the Rx half builds a
/// phon compatibility decode program from this *writer* schema to its own
/// `Message` descriptor (`r[zerocopy.framing.value.decode-plan]`). There is no
/// same-version envelope shortcut — when no schema was exchanged, the writer schema
/// defaults to our own, which is the *schema-identical degenerate output* of the one
/// compat path (the identical `lower_decode`), not a second code path.
pub struct MessagePlan {
    /// The peer's `Message` envelope schema as phon self-describing bytes
    /// (`vox_phon::schema_bytes`). Used lazily in the Rx half against the
    /// concrete message family being decoded.
    pub writer_schema: Vec<u8>,
}

impl MessagePlan {
    /// Build a message plan from the handshake result's schema exchange.
    pub fn from_handshake(result: &vox_types::HandshakeResult) -> Result<Self, String> {
        // `peer_schema` is the phon schema closure of the peer's `Message`. It is
        // empty only in the degenerate no-exchange case; fall back to our own
        // schema so the writer==reader program is still built through the single
        // compat path (NOT a same-version shortcut — the identical `lower_decode`).
        let writer_schema = if result.peer_schema.is_empty() {
            vox_phon::schema_bytes::<vox_types::Message<'static>>().map_err(|e| e.to_string())?
        } else {
            result.peer_schema.clone()
        };
        Ok(MessagePlan { writer_schema })
    }
}

pub mod testing;

#[cfg(test)]
mod tests;
