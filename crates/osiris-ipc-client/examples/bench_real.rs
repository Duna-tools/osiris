//! Mide el round-trip IPC real contra el bridge GDScript de Godot (Fase 6:
//! criterio de avanzar a GDExtension). Requiere un editor Godot con el addon
//! Osiris Bridge activo. Uso:
//!   cargo run -p osiris-ipc-client --example bench_real -- <addr> <token> [n]
use serde_json::json;
use std::time::{Duration, Instant};

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let n: usize = match args.as_slice() {
        [_a, _t] => 200,
        [_a, _t, n] => n.parse().unwrap_or(200),
        _ => {
            eprintln!("uso: bench_real <addr> <token> [n]");
            std::process::exit(2);
        }
    };
    let (addr, token) = (args[0].clone(), args[1].clone());

    let mut client =
        osiris_ipc_client::IpcClient::connect(&addr, &token, Duration::from_secs(5)).unwrap();
    // warm-up: handshake ya hecho; una llamada de calentamiento.
    client.call("ping", json!({})).unwrap();

    let mut samples: Vec<u128> = Vec::with_capacity(n);
    for _ in 0..n {
        let t0 = Instant::now();
        client.call("ping", json!({})).unwrap();
        samples.push(t0.elapsed().as_micros());
    }
    samples.sort_unstable();

    let p50 = samples[n / 2];
    let p95 = samples[(n * 95) / 100];
    let p99 = samples[(n * 99) / 100];
    let total: u128 = samples.iter().sum();
    println!(
        "round-trip IPC real contra el bridge Godot ({} llamadas):\n  p50 = {} us\n  p95 = {} us\n  p99 = {} us\n  media = {} us",
        n,
        p50,
        p95,
        p99,
        total / n as u128,
    );
}
