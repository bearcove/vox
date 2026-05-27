//! Core implementations for the vox connectivity layer.
//!
//! This crate provides concrete implementations of the traits defined in
//! [`vox_types`]. The only conduit shape is [`BareConduit`]: wraps a raw
//! `Link` with binette serialization. No reconnect, no reliability —
//! retry and replay behavior is modeled above the conduit layer.

mod bare_conduit;
pub use bare_conduit::*;
pub use vox_types::TransportMode;

mod handshake;
pub use handshake::*;

mod into_conduit;
pub use into_conduit::*;

mod operation_store;
pub use operation_store::*;

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

/// Pre-built translation plan for deserializing the `Message` wire type.
///
/// Built once from the peer's schema (received during handshake) and our
/// local schema. Stored in the conduit's Rx half and used for every
/// incoming message.
pub struct MessagePlan {
    pub writer_root: binette::TypeRef,
    pub registry: binette::SchemaRegistry,
}

impl MessagePlan {
    fn for_shape(shape: &'static facet_core::Shape) -> Result<Self, String> {
        let writer_plan = binette::writer_plan_for_shape(shape)
            .map_err(|e| format!("failed to build binette writer plan: {e}"))?;
        let mut registry = binette::SchemaRegistry::new();
        registry
            .install_bundle(writer_plan.schema_bundle())
            .map_err(|e| format!("failed to install binette schema bundle: {e}"))?;
        Ok(Self {
            writer_root: writer_plan.root().clone(),
            registry,
        })
    }

    /// Build a message plan from the handshake result's schema exchange.
    pub fn from_handshake(result: &vox_types::HandshakeResult) -> Result<Self, String> {
        let _ = result;
        Self::for_shape(<vox_types::Message<'static> as facet::Facet<'static>>::SHAPE)
    }
}

pub mod testing;

#[cfg(test)]
mod tests;
