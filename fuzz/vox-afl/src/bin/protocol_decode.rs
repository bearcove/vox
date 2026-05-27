use afl::fuzz;
use std::sync::LazyLock;
use vox_types::{Message, MessagePayload, Payload, RequestBody};

struct FuzzCodec {
    writer_plan: binette::WriterPlan,
    registry: binette::SchemaRegistry,
}

static CODEC: LazyLock<FuzzCodec> = LazyLock::new(|| {
    let writer_plan =
        binette::writer_plan_for::<Message<'static>>().expect("build binette message writer plan");
    let mut registry = binette::SchemaRegistry::new();
    registry
        .install_bundle(writer_plan.schema_bundle())
        .expect("install binette message schema bundle");
    FuzzCodec {
        writer_plan,
        registry,
    }
});

fn can_reencode_after_decode(message: &Message<'_>) -> bool {
    match &message.payload {
        MessagePayload::RequestMessage(req) => match &req.body {
            RequestBody::Call(call) => matches!(call.args, Payload::BinetteBytes(_)),
            RequestBody::Response(resp) => matches!(resp.ret, Payload::BinetteBytes(_)),
            RequestBody::Cancel(_) => true,
        },
        MessagePayload::ChannelMessage(ch) => match &ch.body {
            vox_types::ChannelBody::Item(item) => matches!(item.item, Payload::BinetteBytes(_)),
            vox_types::ChannelBody::Close(_) => true,
            vox_types::ChannelBody::Reset(_) => true,
            vox_types::ChannelBody::GrantCredit(_) => true,
        },
        _ => true,
    }
}

fn main() {
    fuzz!(|data: &[u8]| {
        let codec = &*CODEC;
        let Ok(message) = binette::decode_from_slice::<Message<'static>>(
            data,
            codec.writer_plan.root(),
            &codec.registry,
        ) else {
            return;
        };

        if can_reencode_after_decode(&message) {
            let _ = binette::encode_to_vec_with_plan(&message, &codec.writer_plan);
        }
    });
}
