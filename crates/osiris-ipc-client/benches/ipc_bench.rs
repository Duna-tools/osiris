use criterion::{Criterion, criterion_group, criterion_main};
use osiris_ipc_client::IpcClient;
use osiris_ipc_contract::{BridgeMessage, ClientMessage};
use serde_json::json;
use std::io::{BufRead, BufReader, Write};
use std::net::TcpListener;
use std::time::Duration;

fn bench_handshake_and_call(c: &mut Criterion) {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = listener.local_addr().unwrap().to_string();
    std::thread::spawn(move || {
        loop {
            let (stream, _) = listener.accept().unwrap();
            let mut reader = BufReader::new(stream.try_clone().unwrap());
            let mut writer = stream;
            let mut line = String::new();
            reader.read_line(&mut line).unwrap();
            writeln!(
                writer,
                r#"{{"type":"hello_ack","protocol":1,"godot":"4.5.1"}}"#
            )
            .unwrap();
            writer.flush().unwrap();
            loop {
                let mut line = String::new();
                if reader.read_line(&mut line).unwrap() == 0 {
                    break;
                }
                let msg: serde_json::Value = serde_json::from_str(&line).unwrap();
                let id = msg["id"].as_u64().unwrap();
                writeln!(
                    writer,
                    r#"{{"type":"response","id":{},"ok":{{"pong":true}}}}"#,
                    id
                )
                .unwrap();
                writer.flush().unwrap();
            }
        }
    });

    c.bench_function("connect_handshake", |b| {
        b.iter(|| {
            let client = IpcClient::connect(&addr, "tok", Duration::from_secs(1)).unwrap();
            std::hint::black_box(client);
        })
    });

    c.bench_function("call_roundtrip", |b| {
        let mut client = IpcClient::connect(&addr, "tok", Duration::from_secs(1)).unwrap();
        b.iter(|| {
            let result = client.call("ping", json!({})).unwrap();
            std::hint::black_box(result);
        })
    });
}

criterion_group!(benches, bench_handshake_and_call);
criterion_main!(benches);
