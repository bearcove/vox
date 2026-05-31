use std::marker::PhantomData;

use vox_types::{Conduit, ConduitRx, ConduitTx, Link, LinkTx, MaybeSend, MsgFamily, SelfRef};

use crate::MessagePlan;

/// Wraps a [`Link`] with phon serialization. No reconnect, no reliability.
///
/// If the link dies, the conduit is dead. For localhost, SHM, or any
/// transport where reconnect isn't needed.
///
/// `F` is a [`MsgFamily`] — it maps lifetimes to concrete message types.
/// The send path accepts `F::Msg<'a>` (borrowed data serialized in place).
/// The recv path yields `SelfRef<F::Msg<'static>>` (zero-copy: the decoded
/// value borrows the received backing).
///
/// The `Message` envelope is a fixed protocol type — identical on both peers —
/// so it is encoded and decoded single-schema (no translation plan). Schema
/// evolution lives at the *payload* level (the opaque `Payload` field + the
/// `schemas` bindings), not the envelope.
// r[impl conduit.bare]
// r[impl zerocopy.framing.conduit.bare]
pub struct BareConduit<F: MsgFamily, L: Link> {
    link: L,
    _phantom: PhantomData<fn(F) -> F>,
}

impl<F: MsgFamily, L: Link> BareConduit<F, L> {
    /// Create a new BareConduit.
    pub fn new(link: L) -> Self {
        Self {
            link,
            _phantom: PhantomData,
        }
    }

    /// Create a new BareConduit. The `MessagePlan` is vestigial now that the
    /// envelope is decoded single-schema; it is accepted (and dropped) so session
    /// construction sites need not change while the handshake is migrated.
    pub fn with_message_plan(link: L, _message_plan: MessagePlan) -> Self {
        Self::new(link)
    }
}

impl<F: MsgFamily, L: Link> Conduit for BareConduit<F, L>
where
    L::Tx: MaybeSend + 'static,
    L::Rx: MaybeSend + 'static,
{
    type Msg = F;
    type Tx = BareConduitTx<F, L::Tx>;
    type Rx = BareConduitRx<F, L::Rx>;

    fn split(self) -> (Self::Tx, Self::Rx) {
        let (tx, rx) = self.link.split();
        (
            BareConduitTx {
                link_tx: tx,
                _phantom: PhantomData,
            },
            BareConduitRx {
                link_rx: rx,
                pending_fds: vox_types::FrameFds::default(),
                _phantom: PhantomData,
            },
        )
    }
}

// ---------------------------------------------------------------------------
// Tx
// ---------------------------------------------------------------------------

pub struct BareConduitTx<F: MsgFamily, LTx: LinkTx> {
    link_tx: LTx,
    _phantom: PhantomData<fn(F)>,
}

/// A serialized message plus the file descriptors collected while encoding
/// it. The descriptors travel out-of-band via `SCM_RIGHTS`; off-Unix
/// [`FrameFds`](vox_types::FrameFds) is `()`.
pub struct PreparedFrame {
    pub bytes: Vec<u8>,
    pub fds: vox_types::FrameFds,
}

impl<F: MsgFamily, LTx: LinkTx + MaybeSend + 'static> ConduitTx for BareConduitTx<F, LTx> {
    type Msg = F;
    type Prepared = PreparedFrame;
    type Error = BareConduitError;

    // r[impl zerocopy.framing.single-pass]
    // r[impl zerocopy.framing.no-double-serialize]
    // r[impl zerocopy.scatter]
    fn prepare_send(&self, item: F::Msg<'_>) -> Result<Self::Prepared, Self::Error> {
        // Collect any `Fd`s the encoder funnels into the thread-local
        // collector — same install-around-encode shape as the channel
        // binder. Off-Unix this is a pass-through and `fds` is `()`.
        let (encoded, fds) =
            vox_types::collect_fds(|| vox_phon::to_vec(&item).map_err(BareConduitError::Encode));
        Ok(PreparedFrame {
            bytes: encoded?,
            fds,
        })
    }

    async fn send_prepared(&self, prepared: Self::Prepared) -> Result<(), Self::Error> {
        let PreparedFrame { bytes, fds } = prepared;
        if vox_types::frame_fds_len(&fds) > 0 && !self.link_tx.supports_fd_passing() {
            return Err(BareConduitError::Io(std::io::Error::other(
                "message carries file descriptors but the transport \
                 cannot pass them",
            )));
        }
        self.link_tx
            .send_with_fds(bytes, fds)
            .await
            .map_err(BareConduitError::Io)
    }

    async fn close(self) -> std::io::Result<()> {
        self.link_tx.close().await
    }
}

// ---------------------------------------------------------------------------
// Rx
// ---------------------------------------------------------------------------

pub struct BareConduitRx<F: MsgFamily, LRx> {
    link_rx: LRx,
    /// Descriptors that arrived with the most recently `recv`'d frame,
    /// awaiting [`take_frame_fds`](vox_types::ConduitRx::take_frame_fds).
    pending_fds: vox_types::FrameFds,
    _phantom: PhantomData<fn() -> F>,
}

impl<F: MsgFamily, LRx> ConduitRx for BareConduitRx<F, LRx>
where
    LRx: vox_types::LinkRx + MaybeSend + 'static,
{
    type Msg = F;
    type Error = BareConduitError;

    // r[impl zerocopy.recv]
    #[moire::instrument]
    async fn recv(&mut self) -> Result<Option<SelfRef<F::Msg<'static>>>, Self::Error> {
        let backing = match self.link_rx.recv().await.map_err(|error| {
            BareConduitError::Io(std::io::Error::other(format!("link recv failed: {error}")))
        })? {
            Some(b) => b,
            None => return Ok(None),
        };

        // Capture this frame's descriptors. `Payload` only *borrows* its
        // bytes during Message decode — the typed `Fd` is decoded later by
        // the generated stub — so the fds are threaded out via
        // `take_frame_fds` and installed at that decode site, not here.
        self.pending_fds = self.link_rx.take_frame_fds();

        // The envelope is decoded single-schema, zero-copy: the decoded
        // `Message` borrows the backing (payload span, metadata strings).
        // r[impl zerocopy.recv.selfref]
        SelfRef::try_new(backing, |bytes| {
            vox_phon::from_slice_borrowed::<F::Msg<'static>>(bytes)
                .map_err(BareConduitError::Decode)
        })
        .map(Some)
    }

    fn take_frame_fds(&mut self) -> vox_types::FrameFds {
        std::mem::take(&mut self.pending_fds)
    }
}

// ---------------------------------------------------------------------------
// Error
// ---------------------------------------------------------------------------

#[derive(Debug)]
pub enum BareConduitError {
    Encode(vox_phon::Error),
    Decode(vox_phon::Error),
    Io(std::io::Error),
    LinkDead,
}

impl std::fmt::Display for BareConduitError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Encode(e) => write!(f, "encode error: {e}"),
            Self::Decode(e) => write!(f, "decode error: {e}"),
            Self::Io(e) => write!(f, "io error: {e}"),
            Self::LinkDead => write!(f, "link dead"),
        }
    }
}

impl std::error::Error for BareConduitError {}

#[cfg(test)]
mod tests {
    use vox_types::*;

    use super::*;
    use crate::memory_link_pair;

    #[test]
    fn connection_reject_with_nonempty_metadata_round_trips() {
        let rt = tokio::runtime::Builder::new_current_thread()
            .build()
            .unwrap();
        rt.block_on(async { connection_reject_with_nonempty_metadata_inner().await });
    }

    async fn connection_reject_with_nonempty_metadata_inner() {
        let (a, b) = memory_link_pair(64);
        let a_conduit = BareConduit::<MessageFamily, _>::new(a);
        let b_conduit = BareConduit::<MessageFamily, _>::new(b);
        let (a_tx, _a_rx) = a_conduit.split();
        let (_b_tx, mut b_rx) = b_conduit.split();

        // Send a ConnectionReject with non-empty metadata
        let msg = Message {
            connection_id: ConnectionId(1),
            payload: MessagePayload::ConnectionReject(ConnectionReject {
                metadata: metadata()
                    .str("error", "missing required vox-service metadata")
                    .build(),
            }),
        };
        let prepared = a_tx.prepare_send(msg).unwrap();
        a_tx.send_prepared(prepared).await.unwrap();

        // Receive and verify
        let received = b_rx.recv().await.unwrap().unwrap();
        let msg = received.get();
        if let MessagePayload::ConnectionReject(reject) = &msg.payload {
            assert_eq!(reject.metadata.meta_len(), 1, "expected 1 metadata entry");
            assert_eq!(
                reject.metadata.meta_str("error"),
                Some("missing required vox-service metadata"),
            );
        } else {
            panic!("expected ConnectionReject, got {:?}", msg.payload);
        }
    }
}
