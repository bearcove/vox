use std::future::pending;
use std::time::Duration;

use afl::fuzz;
use spec_proto::{Message, Point, TestbedClient, TestbedDispatcher};
use subject_rust::TestbedService;
use vox::{TransportMode, initiator_on, memory_link_pair};

struct Cursor<'a> {
    bytes: &'a [u8],
    idx: usize,
}

impl<'a> Cursor<'a> {
    fn new(bytes: &'a [u8]) -> Self {
        Self { bytes, idx: 0 }
    }

    fn next_u8(&mut self) -> u8 {
        if self.bytes.is_empty() {
            return 0;
        }
        let b = self.bytes[self.idx % self.bytes.len()];
        self.idx = self.idx.wrapping_add(1);
        b
    }

    fn next_u32(&mut self) -> u32 {
        let mut buf = [0u8; 4];
        for b in &mut buf {
            *b = self.next_u8();
        }
        u32::from_le_bytes(buf)
    }

    fn next_i64(&mut self) -> i64 {
        let mut buf = [0u8; 8];
        for b in &mut buf {
            *b = self.next_u8();
        }
        i64::from_le_bytes(buf)
    }

    fn bytes(&mut self, max_len: usize) -> Vec<u8> {
        let len = usize::from(self.next_u8()) % (max_len + 1);
        (0..len).map(|_| self.next_u8()).collect()
    }

    fn string(&mut self, max_len: usize) -> String {
        String::from_utf8_lossy(&self.bytes(max_len)).into_owned()
    }
}

async fn setup_client() -> Option<TestbedClient> {
    let (client_link, server_link) = memory_link_pair(64 * 1024);

    tokio::spawn(async move {
        let Ok(root) = vox::acceptor_on(server_link)
            .on_connection(TestbedDispatcher::new(TestbedService))
            .establish::<vox::NoopClient>()
            .await
        else {
            return;
        };
        let _root = root;
        pending::<()>().await;
    });

    let Ok(client) = initiator_on(client_link, TransportMode::Bare)
        .establish::<TestbedClient>()
        .await
    else {
        return None;
    };

    Some(client)
}

async fn run_case(data: &[u8]) {
    let Some(client) = setup_client().await else {
        return;
    };

    let mut cur = Cursor::new(data);
    let ops = (usize::from(cur.next_u8()) % 24) + 1;

    for _ in 0..ops {
        match cur.next_u8() % 10 {
            0 => {
                let s = cur.string(64);
                let _ = tokio::time::timeout(Duration::from_millis(25), client.echo(s)).await;
            }
            1 => {
                let s = cur.string(64);
                let _ = tokio::time::timeout(Duration::from_millis(25), client.reverse(s)).await;
            }
            2 => {
                let a = cur.next_i64();
                let b = cur.next_i64();
                let _ = tokio::time::timeout(Duration::from_millis(25), client.divide(a, b)).await;
            }
            3 => {
                let id = cur.next_u32();
                let _ = tokio::time::timeout(Duration::from_millis(25), client.lookup(id)).await;
            }
            4 => {
                let payload = cur.bytes(1024);
                if let Ok(Ok(Message::Data(ret))) = tokio::time::timeout(
                    Duration::from_millis(25),
                    client.process_message(Message::Data(payload.clone())),
                )
                .await
                {
                    let mut expected = payload;
                    expected.reverse();
                    assert_eq!(ret, expected);
                }
            }
            5 => {
                let (tx, rx) = vox::channel::<i32>();
                let count = usize::from(cur.next_u8() % 8);
                let mut nums = Vec::with_capacity(count);
                for _ in 0..count {
                    nums.push(i32::from_le_bytes(cur.next_u32().to_le_bytes()));
                }
                tokio::spawn(async move {
                    for n in nums {
                        let _ = tx.send(n).await;
                    }
                    let _ = tx.close(Default::default()).await;
                });
                let _ = tokio::time::timeout(Duration::from_millis(30), client.sum(rx)).await;
            }
            6 => {
                let count = cur.next_u32() % 16;
                let (tx, mut rx) = vox::channel::<i32>();
                let recv_task = tokio::spawn(async move {
                    let mut out = Vec::new();
                    while let Ok(Some(n)) = rx.recv().await {
                        out.push(*n.get());
                        if out.len() > 32 {
                            break;
                        }
                    }
                    out
                });
                let _ = tokio::time::timeout(Duration::from_millis(40), client.generate(count, tx))
                    .await;
                let _ = tokio::time::timeout(Duration::from_millis(40), recv_task).await;
            }
            7 => {
                let (in_tx, in_rx) = vox::channel::<String>();
                let (out_tx, mut out_rx) = vox::channel::<String>();
                let count = usize::from(cur.next_u8() % 6);
                let mut vals = Vec::new();
                for _ in 0..count {
                    vals.push(cur.string(24));
                }
                tokio::spawn(async move {
                    for s in vals {
                        let _ = in_tx.send(s).await;
                    }
                    let _ = in_tx.close(Default::default()).await;
                });
                let recv_task = tokio::spawn(async move {
                    let mut out = Vec::new();
                    while let Ok(Some(s)) = out_rx.recv().await {
                        out.push(s.get().clone());
                        if out.len() > 12 {
                            break;
                        }
                    }
                    out
                });
                let _ = tokio::time::timeout(
                    Duration::from_millis(40),
                    client.transform(in_rx, out_tx),
                )
                .await;
                let _ = tokio::time::timeout(Duration::from_millis(40), recv_task).await;
            }
            8 => {
                let point = Point {
                    x: i32::from_le_bytes(cur.next_u32().to_le_bytes()),
                    y: i32::from_le_bytes(cur.next_u32().to_le_bytes()),
                };
                let _ =
                    tokio::time::timeout(Duration::from_millis(25), client.echo_point(point)).await;
            }
            _ => {
                let pair = (
                    i32::from_le_bytes(cur.next_u32().to_le_bytes()),
                    cur.string(32),
                );
                let _ =
                    tokio::time::timeout(Duration::from_millis(25), client.swap_pair(pair)).await;
            }
        }
    }
}

fn main() {
    fuzz!(|data: &[u8]| {
        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("create runtime");
        rt.block_on(run_case(data));
    });
}
