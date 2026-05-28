use facet::Facet;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

use tokio::sync::Notify;
use vox_types::{
    Backing, ChannelClose, ChannelItem, ChannelReset, ChannelSink, Conduit, ConduitRx, ConduitTx,
    ConnectionId, IncomingChannelMessage, Message, MessageFamily, MessagePayload, Metadata,
    MsgFamily, Payload, RequestBody, RequestId, RequestMessage, RequestResponse, Rx, RxError,
    SelfRef, Tx, TxError,
};

use crate::{
    BareConduit, MemoryLink, TransportMode, accept_transport, initiate_transport, memory_link_pair,
};

mod credit_tests;
mod driver_tests;
pub(crate) mod utils;
mod vconn_tests;

struct StringFamily;

impl MsgFamily for StringFamily {
    type Msg<'a> = String;

    fn shape() -> &'static facet_core::Shape {
        String::SHAPE
    }
}

type StringConduit = BareConduit<StringFamily, MemoryLink>;

/// Create a connected pair of BareConduits over MemoryLink for String messages.
fn conduit_pair() -> (StringConduit, StringConduit) {
    let (a, b) = memory_link_pair(16);
    (BareConduit::new(a), BareConduit::new(b))
}

#[tokio::test]
async fn transport_prologue_accepts_bare_mode() {
    let (client, server) = memory_link_pair(16);
    let acceptor = tokio::spawn(async move { accept_transport(server).await.unwrap().0 });
    let _initiator = initiate_transport(client, TransportMode::Bare)
        .await
        .unwrap();
    assert_eq!(acceptor.await.unwrap(), TransportMode::Bare);
}

#[tokio::test]
async fn send_recv_single() {
    let (client, server) = conduit_pair();
    let (client_tx, _client_rx) = client.split();
    let (_server_tx, mut server_rx) = server.split();

    let prepared = client_tx.prepare_send("hello".to_string()).unwrap();
    client_tx.send_prepared(prepared).await.unwrap();

    let received = server_rx.recv().await.unwrap().unwrap();
    let received = received.get();
    assert_eq!(&**received, "hello");
}

#[tokio::test]
async fn send_recv_multiple_in_order() {
    let (client, server) = conduit_pair();
    let (client_tx, _client_rx) = client.split();
    let (_server_tx, mut server_rx) = server.split();

    for i in 0..10 {
        let prepared = client_tx.prepare_send(format!("msg-{i}")).unwrap();
        client_tx.send_prepared(prepared).await.unwrap();
    }

    for i in 0..10 {
        let received = server_rx.recv().await.unwrap().unwrap();
        let received = received.get();
        assert_eq!(&**received, &format!("msg-{i}"));
    }
}

#[tokio::test]
async fn bidirectional() {
    let (client, server) = conduit_pair();
    let (client_tx, mut client_rx) = client.split();
    let (server_tx, mut server_rx) = server.split();

    let prepared = client_tx.prepare_send("from-client".to_string()).unwrap();
    client_tx.send_prepared(prepared).await.unwrap();

    let received = server_rx.recv().await.unwrap().unwrap();
    let received = received.get();
    assert_eq!(&**received, "from-client");

    let prepared = server_tx.prepare_send("from-server".to_string()).unwrap();
    server_tx.send_prepared(prepared).await.unwrap();

    let received = client_rx.recv().await.unwrap().unwrap();
    let received = received.get();
    assert_eq!(&**received, "from-server");
}

fn decode_binette_payload<T: Facet<'static>>(bytes: &[u8]) -> T {
    let writer_plan = binette::writer_plan_for::<T>().expect("payload writer plan");
    let mut registry = binette::SchemaRegistry::new();
    registry
        .install_bundle(writer_plan.schema_bundle())
        .expect("install payload schema bundle");
    binette::decode_from_slice(bytes, writer_plan.root(), &registry).expect("decode payload")
}

// r[verify zerocopy.framing.value.opaque]
#[tokio::test]
async fn response_value_payload_crosses_bare_conduit_as_binette_bytes() {
    let (client_link, server_link) = memory_link_pair(16);
    let client = BareConduit::<MessageFamily, _>::new(client_link);
    let server = BareConduit::<MessageFamily, _>::new(server_link);
    let (client_tx, _client_rx) = client.split();
    let (_server_tx, mut server_rx) = server.split();

    let ret = 0xFACE_B00C_u64;
    let msg = Message {
        connection_id: ConnectionId::ROOT,
        payload: MessagePayload::RequestMessage(RequestMessage {
            id: RequestId(7),
            body: RequestBody::Response(RequestResponse {
                metadata: Metadata::default(),
                ret: Payload::outgoing(&ret),
                schemas: Default::default(),
            }),
        }),
    };

    let prepared = client_tx.prepare_send(msg).unwrap();
    client_tx.send_prepared(prepared).await.unwrap();

    let received = server_rx.recv().await.unwrap().unwrap();
    let received = received.get();
    let MessagePayload::RequestMessage(request) = &received.payload else {
        panic!("expected request message");
    };
    let RequestBody::Response(response) = &request.body else {
        panic!("expected response body");
    };
    let Payload::BinetteBytes(bytes) = &response.ret else {
        panic!("response payload should arrive as binette bytes");
    };
    assert_eq!(decode_binette_payload::<u64>(bytes), ret);
}

// r[verify zerocopy.framing.value.opaque]
#[tokio::test]
async fn channel_value_payload_crosses_bare_conduit_as_binette_bytes() {
    use vox_types::{ChannelBody, ChannelId, ChannelMessage};

    let (client_link, server_link) = memory_link_pair(16);
    let client = BareConduit::<MessageFamily, _>::new(client_link);
    let server = BareConduit::<MessageFamily, _>::new(server_link);
    let (client_tx, _client_rx) = client.split();
    let (_server_tx, mut server_rx) = server.split();

    let item = String::from("channel item");
    let msg = Message {
        connection_id: ConnectionId::ROOT,
        payload: MessagePayload::ChannelMessage(ChannelMessage {
            id: ChannelId(9),
            body: ChannelBody::Item(ChannelItem {
                item: Payload::outgoing(&item),
            }),
        }),
    };

    let prepared = client_tx.prepare_send(msg).unwrap();
    client_tx.send_prepared(prepared).await.unwrap();

    let received = server_rx.recv().await.unwrap().unwrap();
    let received = received.get();
    let MessagePayload::ChannelMessage(channel) = &received.payload else {
        panic!("expected channel message");
    };
    let ChannelBody::Item(channel_item) = &channel.body else {
        panic!("expected channel item body");
    };
    let Payload::BinetteBytes(bytes) = &channel_item.item else {
        panic!("channel item payload should arrive as binette bytes");
    };
    assert_eq!(decode_binette_payload::<String>(bytes), item);
}

#[tokio::test]
async fn close_signals_end() {
    let (client, server) = conduit_pair();
    let (client_tx, _client_rx) = client.split();
    let (_server_tx, mut server_rx) = server.split();

    let prepared = client_tx.prepare_send("last".to_string()).unwrap();
    client_tx.send_prepared(prepared).await.unwrap();
    client_tx.close().await.unwrap();

    let received = server_rx.recv().await.unwrap().unwrap();
    let received = received.get();
    assert_eq!(&**received, "last");

    let end = server_rx.recv().await.unwrap();
    assert!(end.is_none());
}

#[tokio::test]
async fn interleaved_send_recv() {
    let (client, server) = conduit_pair();
    let (client_tx, mut client_rx) = client.split();
    let (server_tx, mut server_rx) = server.split();

    for i in 0..5 {
        let prepared = client_tx.prepare_send(format!("c2s-{i}")).unwrap();
        client_tx.send_prepared(prepared).await.unwrap();

        let received = server_rx.recv().await.unwrap().unwrap();
        let received = received.get();
        assert_eq!(&**received, &format!("c2s-{i}"));

        let prepared = server_tx.prepare_send(format!("s2c-{i}")).unwrap();
        server_tx.send_prepared(prepared).await.unwrap();

        let received = client_rx.recv().await.unwrap().unwrap();
        let received = received.get();
        assert_eq!(&**received, &format!("s2c-{i}"));
    }
}

struct TestSink {
    gate: Arc<Notify>,
    send_count: Arc<AtomicUsize>,
    close_count: Arc<AtomicUsize>,
    saw_owned_payload: Arc<AtomicBool>,
}

struct DropCloseSink {
    close_count: Arc<AtomicUsize>,
}

impl DropCloseSink {
    fn new() -> Self {
        Self {
            close_count: Arc::new(AtomicUsize::new(0)),
        }
    }
}

impl ChannelSink for DropCloseSink {
    fn send_payload<'a>(
        &self,
        _payload: Payload<'a>,
    ) -> std::pin::Pin<Box<dyn vox_types::MaybeSendFuture<Output = Result<(), TxError>> + 'a>> {
        Box::pin(async { Ok(()) })
    }

    fn close_channel(
        &self,
        _metadata: Metadata,
    ) -> std::pin::Pin<Box<dyn vox_types::MaybeSendFuture<Output = Result<(), TxError>> + 'static>>
    {
        let close_count = self.close_count.clone();
        Box::pin(async move {
            close_count.fetch_add(1, Ordering::SeqCst);
            Ok(())
        })
    }

    fn close_channel_on_drop(&self) {
        self.close_count.fetch_add(1, Ordering::SeqCst);
    }
}

impl TestSink {
    fn new() -> Self {
        Self {
            gate: Arc::new(Notify::new()),
            send_count: Arc::new(AtomicUsize::new(0)),
            close_count: Arc::new(AtomicUsize::new(0)),
            saw_owned_payload: Arc::new(AtomicBool::new(false)),
        }
    }

    fn open_gate(&self) {
        self.gate.notify_waiters();
    }
}

impl ChannelSink for TestSink {
    fn send_payload<'a>(
        &self,
        payload: Payload<'a>,
    ) -> std::pin::Pin<Box<dyn vox_types::MaybeSendFuture<Output = Result<(), TxError>> + 'a>> {
        let gate = self.gate.clone();
        let send_count = self.send_count.clone();
        let saw_owned_payload = self.saw_owned_payload.clone();
        Box::pin(async move {
            if matches!(payload, Payload::Value { .. }) {
                saw_owned_payload.store(true, Ordering::SeqCst);
            }
            gate.notified().await;
            let _ = payload;
            send_count.fetch_add(1, Ordering::SeqCst);
            Ok(())
        })
    }

    fn close_channel(
        &self,
        metadata: Metadata,
    ) -> std::pin::Pin<Box<dyn vox_types::MaybeSendFuture<Output = Result<(), TxError>> + 'static>>
    {
        let gate = self.gate.clone();
        let close_count = self.close_count.clone();
        Box::pin(async move {
            gate.notified().await;
            let _ = metadata;
            close_count.fetch_add(1, Ordering::SeqCst);
            Ok(())
        })
    }
}

#[tokio::test]
async fn tx_send_waits_for_sink_completion() {
    let mut tx = Tx::<String>::unbound();
    let sink = Arc::new(TestSink::new());
    tx.bind(sink.clone());

    let payload = "hello".to_string();
    let fut = tx.send(payload);
    tokio::pin!(fut);
    tokio::select! {
        res = &mut fut => panic!("send completed too early: {res:?}"),
        _ = tokio::time::sleep(std::time::Duration::from_millis(20)) => {}
    }

    sink.open_gate();
    fut.await.expect("send should complete once sink opens");
    assert_eq!(sink.send_count.load(Ordering::SeqCst), 1);
}

#[tokio::test]
async fn tx_drop_triggers_close_signal() {
    let mut tx = Tx::<u32>::unbound();
    let sink = Arc::new(DropCloseSink::new());
    tx.bind(sink.clone());

    drop(tx);
    tokio::time::sleep(std::time::Duration::from_millis(20)).await;

    assert_eq!(
        sink.close_count.load(Ordering::SeqCst),
        1,
        "dropping Tx should emit channel close"
    );
}

#[tokio::test]
async fn tx_close_then_drop_emits_single_close_signal() {
    let mut tx = Tx::<u32>::unbound();
    let sink = Arc::new(DropCloseSink::new());
    tx.bind(sink.clone());

    tx.close(Metadata::default())
        .await
        .expect("explicit close should succeed");
    drop(tx);
    tokio::time::sleep(std::time::Duration::from_millis(20)).await;

    assert_eq!(
        sink.close_count.load(Ordering::SeqCst),
        1,
        "drop after explicit close should not emit duplicate close"
    );
}

#[derive(Facet)]
struct BorrowedMsg<'a> {
    text: &'a str,
}

#[tokio::test]
async fn tx_send_accepts_borrowed_payloads() {
    let mut tx: Tx<BorrowedMsg<'_>> = Tx::unbound();
    let sink = Arc::new(TestSink::new());
    tx.bind(sink.clone());

    let backing = String::from("borrowed");
    let msg = BorrowedMsg { text: &backing };
    let fut = tx.send(msg);
    tokio::pin!(fut);
    tokio::select! {
        res = &mut fut => panic!("send completed too early: {res:?}"),
        _ = tokio::time::sleep(std::time::Duration::from_millis(20)) => {}
    }
    sink.open_gate();
    fut.await.expect("borrowed send should succeed");
    assert!(
        sink.saw_owned_payload.load(Ordering::SeqCst),
        "send(value) should route through Payload::Owned"
    );
}

// r[verify rpc.channel.item]
// r[verify rpc.channel.close]
#[tokio::test]
async fn rx_recv_decodes_channel_items() {
    let mut rx = Rx::<u32>::unbound();
    let (tx_items, rx_items) = vox_types::channel_mailbox("vox_core.tests.rx_recv_items", 4);
    rx.bind(rx_items);

    let payload_bytes = binette::encode_to_vec(&42_u32).expect("serialize channel item");
    let backing = Backing::Boxed(payload_bytes.into_boxed_slice());
    let item_ref = SelfRef::try_new(backing, |bytes| {
        Ok::<_, std::convert::Infallible>(ChannelItem {
            item: Payload::BinetteBytes(bytes),
        })
    })
    .unwrap();
    tx_items
        .send(IncomingChannelMessage::Item(item_ref))
        .await
        .expect("send item to rx");

    let received = rx.recv().await.expect("recv data");
    let received = received.unwrap();
    let received = received.get();
    assert_eq!(*received, 42);

    let close = ChannelClose {
        metadata: Metadata::default(),
    };
    let close_ref = SelfRef::owning(Backing::Boxed(Box::<[u8]>::default()), close);
    tx_items
        .send(IncomingChannelMessage::Close(close_ref))
        .await
        .expect("send close to rx");
    assert!(rx.recv().await.expect("recv close").is_none());
}

// r[verify rpc.channel.reset]
#[tokio::test]
async fn rx_recv_signals_reset() {
    let mut rx = Rx::<u32>::unbound();
    let (tx_items, rx_items) = vox_types::channel_mailbox("vox_core.tests.rx_recv_reset", 4);
    rx.bind(rx_items);

    let reset = ChannelReset {
        metadata: Metadata::default(),
    };
    let reset_ref = SelfRef::owning(Backing::Boxed(Box::<[u8]>::default()), reset);
    tx_items
        .send(IncomingChannelMessage::Reset(reset_ref))
        .await
        .expect("send reset to rx");

    let result = rx.recv().await;
    assert!(
        matches!(result, Err(RxError::Reset)),
        "expected RxError::Reset"
    );
}
