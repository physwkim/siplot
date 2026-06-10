//! `ca://` round-trip against an in-process EPICS Channel Access IOC.
//!
//! Brings up an [`epics_ca_rs`] `CaServer` on a loopback port holding one
//! DOUBLE PV, points the engine's lazily-created CA client at it via
//! `EPICS_CA_ADDR_LIST`, and drives the full `Engine::connect` → monitor → put
//! path. No external IOC is required; the server runs in this process.
//!
//! Process isolation: `cargo nextest` runs each test in its own process, so the
//! `EPICS_CA_*` env vars set here do not leak to other tests. (No other test in
//! this crate reads them, so this also holds under `cargo test`.)

#![cfg(feature = "ca")]

use std::time::{Duration, Instant};

use epics_ca_rs::EpicsValue;
use epics_ca_rs::server::CaServer;
use sidm::{Engine, PvValue};

/// Poll `cond` until it holds or `timeout` elapses; returns the final result.
fn wait_for(mut cond: impl FnMut() -> bool, timeout: Duration) -> bool {
    let start = Instant::now();
    while start.elapsed() < timeout {
        if cond() {
            return true;
        }
        std::thread::sleep(Duration::from_millis(10));
    }
    cond()
}

/// Reserve then release a free localhost TCP port for the `CaServer` to bind.
fn free_port() -> u16 {
    let probe = std::net::TcpListener::bind(("127.0.0.1", 0)).expect("reserve free CA server port");
    let port = probe.local_addr().unwrap().port();
    drop(probe);
    port
}

#[test]
fn ca_roundtrip_monitor_and_put() {
    let port = free_port();

    // The server is async; host it on a dedicated runtime kept alive for the
    // whole test. `Engine::new()` builds its OWN runtime, so it must not run
    // inside this one — `block_on` is only used for setup, then we are back on
    // a plain thread before the engine is created.
    let server_rt = tokio::runtime::Runtime::new().expect("server runtime");
    let server = server_rt.block_on(async {
        CaServer::builder()
            .port(port)
            .pv("sidm:test:ao", EpicsValue::Double(1.5))
            .build()
            .await
            .expect("build in-process CA server")
    });
    server_rt.spawn(async move {
        let _ = server.run().await;
    });
    // Let the server bind its TCP/UDP sockets before the client searches.
    std::thread::sleep(Duration::from_millis(300));

    // Point the (lazily created) CA client at exactly this server, skipping UDP
    // broadcast search. SAFETY: a single CA test process (nextest isolation),
    // env set before the engine spawns the client task that snapshots the
    // resolver configuration in `CaClient::new`.
    unsafe {
        std::env::set_var("EPICS_CA_ADDR_LIST", format!("127.0.0.1:{port}"));
        std::env::set_var("EPICS_CA_AUTO_ADDR_LIST", "NO");
        std::env::set_var("EPICS_CA_SERVER_PORT", port.to_string());
    }

    let engine = Engine::new();
    let ch = engine
        .connect("ca://sidm:test:ao")
        .expect("connect ca channel");

    assert!(
        wait_for(|| ch.is_connected(), Duration::from_secs(5)),
        "channel never connected to the in-process IOC"
    );

    // The metadata fetch / initial monitor delivers the seeded value.
    assert!(
        wait_for(
            || matches!(ch.read(|s| s.value.clone()), Some(PvValue::Float(v)) if (v - 1.5).abs() < 1e-9),
            Duration::from_secs(5)
        ),
        "did not observe the seeded value 1.5 (got {:?})",
        ch.read(|s| s.value.clone())
    );

    // Write back through the GUI→engine queue and observe the echo via monitor.
    ch.put(PvValue::Float(2.5));
    assert!(
        wait_for(
            || matches!(ch.read(|s| s.value.clone()), Some(PvValue::Float(v)) if (v - 2.5).abs() < 1e-9),
            Duration::from_secs(5)
        ),
        "did not observe the written value 2.5 (got {:?})",
        ch.read(|s| s.value.clone())
    );

    // Dropping the last Channel cancels the connection task; the engine and its
    // runtime drop here, then the server runtime stops the IOC.
    drop(ch);
    drop(engine);
    drop(server_rt);
}
