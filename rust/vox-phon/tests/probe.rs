//! End-to-end validation that phon can carry vox's `Message` envelope: the
//! opaque `Payload` field, `Cow` metadata, transparent id newtypes, and borrowed
//! (zero-copy) decode of the whole thing.

use facet::Facet;
use vox_types::*;

fn probe<T: Facet<'static>>(name: &str) {
    match phon::derive::of::<T>() {
        Ok(d) => println!("OK   {name}: root={:?} schemas={}", d.root, d.schemas.len()),
        Err(e) => println!("FAIL {name}: {e}"),
    }
}

#[test]
fn probe_vox_wire_types() {
    probe::<ConnectionId>("ConnectionId");
    probe::<MetadataValue>("MetadataValue");
    probe::<MetadataEntry>("MetadataEntry");
    probe::<RequestCall>("RequestCall");
    probe::<MessagePayload>("MessagePayload");
    probe::<Message>("Message");
}

/// A full `Message` (RequestCall carrying an inline `Payload::Value`) round-trips
/// through phon: encode the envelope (opaque payload sub-encoded inline), then
/// borrowed-decode it back. The payload becomes a zero-copy span pointing INTO the
/// wire, metadata strings borrow the wire, and the span re-decodes to the args.
#[test]
fn message_with_value_payload_roundtrips() {
    let args: u32 = 42;
    let msg = Message {
        connection_id: ConnectionId(1),
        payload: MessagePayload::RequestMessage(RequestMessage {
            id: RequestId(7),
            body: RequestBody::Call(RequestCall {
                method_id: MethodId(0xABCD),
                metadata: vec![
                    MetadataEntry::str("trace", "abc"),
                    MetadataEntry::u64("n", 99),
                ],
                args: Payload::outgoing(&args),
                schemas: CborPayload::default(),
            }),
        }),
    };

    let bytes = vox_phon::to_vec(&msg).expect("encode Message");

    let decoded: Message = vox_phon::from_slice_borrowed(&bytes).expect("decode Message");
    assert_eq!(decoded.connection_id, ConnectionId(1));

    let MessagePayload::RequestMessage(rm) = &decoded.payload else {
        panic!("expected RequestMessage, got {:?}", decoded.payload);
    };
    assert_eq!(rm.id, RequestId(7));

    let RequestBody::Call(call) = &rm.body else {
        panic!("expected Call");
    };
    assert_eq!(call.method_id, MethodId(0xABCD));

    // Metadata: Cow keys/values borrow the wire (zero-copy).
    assert_eq!(call.metadata.len(), 2);
    assert_eq!(call.metadata[0].key.as_ref(), "trace");
    match &call.metadata[0].value {
        MetadataValue::String(s) => assert_eq!(s.as_ref(), "abc"),
        other => panic!("expected String metadata, got {other:?}"),
    }
    match &call.metadata[1].value {
        MetadataValue::U64(n) => assert_eq!(*n, 99),
        other => panic!("expected U64 metadata, got {other:?}"),
    }

    // The opaque payload decoded to a borrowed span pointing INTO the wire.
    let Payload::PostcardBytes(span) = &call.args else {
        panic!("expected a borrowed payload span");
    };
    let wire_start = bytes.as_ptr() as usize;
    assert!(
        (wire_start..wire_start + bytes.len()).contains(&(span.as_ptr() as usize)),
        "payload span must point into the wire buffer (zero-copy)"
    );

    // And the span re-decodes to the original args.
    let back: u32 = vox_phon::from_slice(span).expect("decode payload span");
    assert_eq!(back, 42);
}
