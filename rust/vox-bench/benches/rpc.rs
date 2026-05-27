use std::future::pending;

use divan::{Bencher, black_box};
use facet::Facet;
use spec_proto::{GnarlyPayload, TestbedClient, TestbedDispatcher};
use subject_rust::TestbedService;
use tokio::net::{TcpListener, TcpStream};
use tokio::runtime::{Builder, Runtime};
use tokio::sync::oneshot;
use tokio::task::JoinHandle;
use vox::transport::tcp::StreamLink;
use vox::{TransportMode, initiator_on, memory_link_pair};
use vox_bench::make_gnarly_payload;
use vox_types::VoxError;

#[cfg(feature = "mimalloc")]
#[global_allocator]
static GLOBAL: mimalloc::MiMalloc = mimalloc::MiMalloc;

fn main() {
    divan::main();
}

struct RpcHarness {
    rt: Runtime,
    client: TestbedClient,
    server_task: JoinHandle<()>,
}

impl RpcHarness {
    fn mem() -> Self {
        let rt = Builder::new_current_thread().enable_all().build().unwrap();
        let (client, server_task) = rt.block_on(async {
            let (client_link, server_link) = memory_link_pair(64);
            let (ready_tx, ready_rx) = oneshot::channel::<Result<(), String>>();

            let server_task = tokio::spawn(async move {
                let root = vox::acceptor_on(server_link)
                    .on_connection(TestbedDispatcher::new(TestbedService))
                    .establish::<vox::NoopClient>()
                    .await
                    .map_err(|e| format!("server establish: {e}"));

                match root {
                    Ok(root) => {
                        let _ = ready_tx.send(Ok(()));
                        let _root = root;
                        pending::<()>().await;
                    }
                    Err(err) => {
                        let _ = ready_tx.send(Err(err));
                    }
                }
            });

            let client = initiator_on(client_link, TransportMode::Bare)
                .establish::<TestbedClient>()
                .await
                .expect("client establish");

            ready_rx.await.expect("server ready").expect("server setup");
            (client, server_task)
        });

        Self {
            rt,
            client,
            server_task,
        }
    }

    fn tcp() -> Self {
        let rt = Builder::new_current_thread().enable_all().build().unwrap();
        let (client, server_task) = rt.block_on(async {
            let listener = TcpListener::bind("127.0.0.1:0").await.expect("bind");
            let addr = listener.local_addr().expect("local_addr");
            let (ready_tx, ready_rx) = oneshot::channel::<Result<(), String>>();

            let server_task = tokio::spawn(async move {
                let (stream, _) = listener.accept().await.expect("accept");
                stream.set_nodelay(true).expect("set_nodelay");
                let root = vox::acceptor_on(StreamLink::tcp(stream))
                    .on_connection(TestbedDispatcher::new(TestbedService))
                    .establish::<vox::NoopClient>()
                    .await
                    .map_err(|e| format!("server establish: {e}"));

                match root {
                    Ok(root) => {
                        let _ = ready_tx.send(Ok(()));
                        let _root = root;
                        pending::<()>().await;
                    }
                    Err(err) => {
                        let _ = ready_tx.send(Err(err));
                    }
                }
            });

            let client_stream = TcpStream::connect(addr).await.expect("connect");
            client_stream.set_nodelay(true).expect("set_nodelay");

            let client = initiator_on(StreamLink::tcp(client_stream), TransportMode::Bare)
                .establish::<TestbedClient>()
                .await
                .expect("client establish");

            ready_rx.await.expect("server ready").expect("server setup");
            (client, server_task)
        });

        Self {
            rt,
            client,
            server_task,
        }
    }

    fn echo_u64(&self, value: u64) -> u64 {
        self.rt
            .block_on(self.client.echo_u64(value))
            .expect("echo_u64 call")
    }

    fn echo_gnarly(&self, payload: GnarlyPayload) -> GnarlyPayload {
        self.rt
            .block_on(self.client.echo_gnarly(payload))
            .expect("echo_gnarly call")
    }
}

impl Drop for RpcHarness {
    fn drop(&mut self) {
        self.server_task.abort();
    }
}

type GnarlyArgs = (GnarlyPayload,);
type GnarlyResponse = Result<GnarlyPayload, VoxError<std::convert::Infallible>>;

struct BinetteFixture<T> {
    value: T,
    bytes: Vec<u8>,
    writer_plan: binette::WriterPlan,
    registry: binette::SchemaRegistry,
}

impl<T> BinetteFixture<T>
where
    T: Facet<'static>,
{
    fn new(value: T) -> Self {
        let writer_plan = binette::writer_plan_for::<T>().expect("binette writer plan");
        let mut registry = binette::SchemaRegistry::new();
        registry
            .install_bundle(writer_plan.schema_bundle())
            .expect("install binette schema bundle");
        let bytes =
            binette::encode_to_vec_with_plan(&value, &writer_plan).expect("binette encode fixture");
        let _: T = binette::decode_from_slice(&bytes, writer_plan.root(), &registry)
            .expect("binette decode fixture");
        Self {
            value,
            bytes,
            writer_plan,
            registry,
        }
    }
}

mod codec {
    use super::*;

    fn interp_encode<T: Facet<'static>>(bencher: Bencher, fixture: &BinetteFixture<T>) {
        bencher.bench_local(|| {
            black_box(
                binette::encode_to_vec_with_plan(black_box(&fixture.value), &fixture.writer_plan)
                    .unwrap(),
            )
        });
    }

    fn stencil_encode<T: Facet<'static>>(bencher: Bencher, fixture: &BinetteFixture<T>) {
        let encoder = binette::hybrid_stencil_encoder_from_plan::<T>(&fixture.writer_plan)
            .expect("binette stencil encoder");
        bencher.bench_local(|| {
            black_box(
                binette::encode_to_vec_with_stencil(black_box(&fixture.value), &encoder).unwrap(),
            )
        });
    }

    fn interp_decode<T: Facet<'static>>(bencher: Bencher, fixture: &BinetteFixture<T>) {
        bencher.bench_local(|| {
            black_box(
                binette::decode_from_slice::<T>(
                    black_box(&fixture.bytes),
                    fixture.writer_plan.root(),
                    &fixture.registry,
                )
                .unwrap(),
            )
        });
    }

    fn stencil_decode<T: Facet<'static>>(bencher: Bencher, fixture: &BinetteFixture<T>) {
        let decoder =
            binette::hybrid_stencil_decoder_for::<T>(fixture.writer_plan.root(), &fixture.registry)
                .expect("binette stencil decoder");
        bencher.bench_local(|| {
            black_box(
                binette::decode_from_slice_with_stencil::<T>(black_box(&fixture.bytes), &decoder)
                    .unwrap(),
            )
        });
    }

    mod args {
        use super::*;

        #[divan::bench(args = [1, 4, 16])]
        fn interp_encode(bencher: Bencher, n: usize) {
            let fixture = BinetteFixture::new((make_gnarly_payload(n, 0),));
            super::interp_encode(bencher, &fixture);
        }

        #[divan::bench(args = [1, 4, 16])]
        fn stencil_encode(bencher: Bencher, n: usize) {
            let fixture = BinetteFixture::new((make_gnarly_payload(n, 0),));
            super::stencil_encode(bencher, &fixture);
        }

        #[divan::bench(args = [1, 4, 16])]
        fn interp_decode(bencher: Bencher, n: usize) {
            let fixture = BinetteFixture::<GnarlyArgs>::new((make_gnarly_payload(n, 0),));
            super::interp_decode(bencher, &fixture);
        }

        #[divan::bench(args = [1, 4, 16])]
        fn stencil_decode(bencher: Bencher, n: usize) {
            let fixture = BinetteFixture::<GnarlyArgs>::new((make_gnarly_payload(n, 0),));
            super::stencil_decode(bencher, &fixture);
        }
    }

    mod response {
        use super::*;

        #[divan::bench(args = [1, 4, 16])]
        fn interp_encode(bencher: Bencher, n: usize) {
            let fixture = BinetteFixture::new(Ok::<_, VoxError<std::convert::Infallible>>(
                make_gnarly_payload(n, 0),
            ));
            super::interp_encode(bencher, &fixture);
        }

        #[divan::bench(args = [1, 4, 16])]
        fn stencil_encode(bencher: Bencher, n: usize) {
            let fixture = BinetteFixture::new(Ok::<_, VoxError<std::convert::Infallible>>(
                make_gnarly_payload(n, 0),
            ));
            super::stencil_encode(bencher, &fixture);
        }

        #[divan::bench(args = [1, 4, 16])]
        fn interp_decode(bencher: Bencher, n: usize) {
            let fixture = BinetteFixture::<GnarlyResponse>::new(Ok(make_gnarly_payload(n, 0)));
            super::interp_decode(bencher, &fixture);
        }

        #[divan::bench(args = [1, 4, 16])]
        fn stencil_decode(bencher: Bencher, n: usize) {
            let fixture = BinetteFixture::<GnarlyResponse>::new(Ok(make_gnarly_payload(n, 0)));
            super::stencil_decode(bencher, &fixture);
        }
    }

    mod wide_struct {
        use super::*;
        use vox_bench::shapes::{WideStruct, make_wide};

        #[divan::bench]
        fn interp_encode(bencher: Bencher) {
            let fixture = BinetteFixture::new(make_wide(0xDEAD_BEEF));
            super::interp_encode(bencher, &fixture);
        }

        #[divan::bench]
        fn stencil_encode(bencher: Bencher) {
            let fixture = BinetteFixture::new(make_wide(0xDEAD_BEEF));
            super::stencil_encode(bencher, &fixture);
        }

        #[divan::bench]
        fn interp_decode(bencher: Bencher) {
            let fixture = BinetteFixture::<WideStruct>::new(make_wide(0xDEAD_BEEF));
            super::interp_decode(bencher, &fixture);
        }

        #[divan::bench]
        fn stencil_decode(bencher: Bencher) {
            let fixture = BinetteFixture::<WideStruct>::new(make_wide(0xDEAD_BEEF));
            super::stencil_decode(bencher, &fixture);
        }
    }

    mod many_variants {
        use super::*;
        use vox_bench::shapes::{ManyVariants, make_many_variants};

        #[divan::bench(args = [0u32, 1, 7, 9, 11, 15])]
        fn interp_encode(bencher: Bencher, variant: u32) {
            let fixture = BinetteFixture::new(make_many_variants(variant));
            super::interp_encode(bencher, &fixture);
        }

        #[divan::bench(args = [0u32, 1, 7, 9, 11, 15])]
        fn stencil_encode(bencher: Bencher, variant: u32) {
            let fixture = BinetteFixture::new(make_many_variants(variant));
            super::stencil_encode(bencher, &fixture);
        }

        #[divan::bench(args = [0u32, 1, 7, 9, 11, 15])]
        fn interp_decode(bencher: Bencher, variant: u32) {
            let fixture = BinetteFixture::<ManyVariants>::new(make_many_variants(variant));
            super::interp_decode(bencher, &fixture);
        }

        #[divan::bench(args = [0u32, 1, 7, 9, 11, 15])]
        fn stencil_decode(bencher: Bencher, variant: u32) {
            let fixture = BinetteFixture::<ManyVariants>::new(make_many_variants(variant));
            super::stencil_decode(bencher, &fixture);
        }
    }

    mod tree {
        use super::*;
        use vox_bench::shapes::{Tree, make_tree};

        #[divan::bench(args = [4u32, 6, 8])]
        fn interp_encode(bencher: Bencher, depth: u32) {
            let fixture = BinetteFixture::new(make_tree(depth, 0xC0FFEE));
            super::interp_encode(bencher, &fixture);
        }

        #[divan::bench(args = [4u32, 6, 8])]
        fn stencil_encode(bencher: Bencher, depth: u32) {
            let fixture = BinetteFixture::new(make_tree(depth, 0xC0FFEE));
            super::stencil_encode(bencher, &fixture);
        }

        #[divan::bench(args = [4u32, 6, 8])]
        fn interp_decode(bencher: Bencher, depth: u32) {
            let fixture = BinetteFixture::<Tree>::new(make_tree(depth, 0xC0FFEE));
            super::interp_decode(bencher, &fixture);
        }

        #[divan::bench(args = [4u32, 6, 8])]
        fn stencil_decode(bencher: Bencher, depth: u32) {
            let fixture = BinetteFixture::<Tree>::new(make_tree(depth, 0xC0FFEE));
            super::stencil_decode(bencher, &fixture);
        }
    }

    mod numeric_buffer {
        use super::*;
        use vox_bench::shapes::{NumericBuffer, make_numeric_buffer};

        #[divan::bench(args = [64usize, 256, 1024])]
        fn interp_encode(bencher: Bencher, n: usize) {
            let fixture = BinetteFixture::new(make_numeric_buffer(n, 0));
            super::interp_encode(bencher, &fixture);
        }

        #[divan::bench(args = [64usize, 256, 1024])]
        fn stencil_encode(bencher: Bencher, n: usize) {
            let fixture = BinetteFixture::new(make_numeric_buffer(n, 0));
            super::stencil_encode(bencher, &fixture);
        }

        #[divan::bench(args = [64usize, 256, 1024])]
        fn interp_decode(bencher: Bencher, n: usize) {
            let fixture = BinetteFixture::<NumericBuffer>::new(make_numeric_buffer(n, 0));
            super::interp_decode(bencher, &fixture);
        }

        #[divan::bench(args = [64usize, 256, 1024])]
        fn stencil_decode(bencher: Bencher, n: usize) {
            let fixture = BinetteFixture::<NumericBuffer>::new(make_numeric_buffer(n, 0));
            super::stencil_decode(bencher, &fixture);
        }
    }
}

fn bench_echo_u64(bencher: Bencher, harness: fn() -> RpcHarness) {
    let harness = harness();
    assert_eq!(harness.echo_u64(7), 7);

    bencher.bench_local(|| black_box(harness.echo_u64(42)));
}

fn bench_echo_gnarly(bencher: Bencher, harness: fn() -> RpcHarness, n: usize) {
    let harness = harness();
    let probe = make_gnarly_payload(n, 0);
    let probe_response = harness.echo_gnarly(probe.clone());
    assert_eq!(probe_response, probe);

    let mut seq = 1usize;
    bencher
        .with_inputs(|| {
            let payload = make_gnarly_payload(n, seq);
            seq += 1;
            payload
        })
        .bench_local_values(|payload| black_box(harness.echo_gnarly(payload)));
}

mod mem {
    use super::*;

    #[divan::bench]
    fn echo_u64(bencher: Bencher) {
        bench_echo_u64(bencher, RpcHarness::mem);
    }

    #[divan::bench(args = [1, 4, 16])]
    fn echo_gnarly(bencher: Bencher, n: usize) {
        bench_echo_gnarly(bencher, RpcHarness::mem, n);
    }
}

mod tcp {
    use super::*;

    #[divan::bench]
    fn echo_u64(bencher: Bencher) {
        bench_echo_u64(bencher, RpcHarness::tcp);
    }

    #[divan::bench(args = [1, 4, 16])]
    fn echo_gnarly(bencher: Bencher, n: usize) {
        bench_echo_gnarly(bencher, RpcHarness::tcp, n);
    }
}
