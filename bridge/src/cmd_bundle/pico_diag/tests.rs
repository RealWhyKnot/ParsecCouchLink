use super::*;

fn make_peer() -> SocketAddr {
    "10.0.0.24:4242".parse().unwrap()
}

#[test]
fn discriminant_strings_are_distinct() {
    let variants = [
        DiagOutcome::Captured {
            source: DiagSource::SetupCdc,
            text: String::new(),
            lost: 0,
        },
        DiagOutcome::Empty {
            source: DiagSource::SetupCdc,
        },
        DiagOutcome::NoSetupPort,
        DiagOutcome::SetupOpenFailed { error: "x".into() },
        DiagOutcome::SetupProbeFailed {
            port: "COM3".into(),
            step: "read",
            elapsed_ms: 0,
            bytes_received: 0,
            rx_first_32_hex: "none".into(),
            error: "x".into(),
        },
        DiagOutcome::NoLastPicoInConfig,
        DiagOutcome::UdpDiscoveryFailed { reason: "x".into() },
        DiagOutcome::UdpProbeFailed {
            peer: make_peer(),
            step: "send_get_log",
            elapsed_ms: 0,
            chunks_received: 0,
            error: "x".into(),
        },
        DiagOutcome::UdpUnsupported { peer: make_peer() },
    ];
    let mut seen = std::collections::HashSet::new();
    for v in &variants {
        let d = v.discriminant_str();
        assert!(seen.insert(d), "duplicate discriminant: {d}");
    }
    assert_eq!(seen.len(), variants.len());
}

#[test]
fn captured_stub_includes_text_and_source() {
    let out = DiagOutcome::Captured {
        source: DiagSource::SetupCdc,
        text: "BOOT: hello".into(),
        lost: 0,
    };
    let stub = out.stub_text();
    assert!(
        stub.contains("setup-mode USB-CDC"),
        "missing source: {stub}"
    );
    assert!(stub.contains("BOOT: hello"), "missing text: {stub}");
}

#[test]
fn captured_stub_flags_lost_bytes() {
    let out = DiagOutcome::Captured {
        source: DiagSource::RunUdp { peer: make_peer() },
        text: "tail-of-ring".into(),
        lost: 1234,
    };
    let stub = out.stub_text();
    assert!(
        stub.contains("1234 byte(s) dropped"),
        "missing lost: {stub}"
    );
    assert!(stub.contains("10.0.0.24:4242"), "missing peer: {stub}");
}

#[test]
fn setup_probe_failed_names_step_and_bytes() {
    let out = DiagOutcome::SetupProbeFailed {
        port: "COM3".into(),
        step: "read",
        elapsed_ms: 3012,
        bytes_received: 0,
        rx_first_32_hex: "none".into(),
        error: "timed out".into(),
    };
    let stub = out.stub_text();
    assert!(
        stub.contains("=== Suggested next step ==="),
        "no header: {stub}"
    );
    assert!(stub.contains("Try this (in order):"), "no try list: {stub}");
    assert!(
        stub.contains("=== Diagnostic details ==="),
        "no detail block: {stub}"
    );
    assert!(stub.contains("port: COM3"), "missing port field: {stub}");
    assert!(stub.contains("step: read"), "missing step field: {stub}");
    assert!(
        stub.contains("elapsed_ms: 3012"),
        "missing elapsed field: {stub}"
    );
    assert!(
        stub.contains("bytes_received: 0"),
        "missing bytes_received field: {stub}"
    );
    assert!(
        stub.contains("did not write a single byte"),
        "missing read+0 lead: {stub}"
    );
}

#[test]
fn udp_unsupported_names_peer() {
    let out = DiagOutcome::UdpUnsupported { peer: make_peer() };
    let stub = out.stub_text();
    assert!(stub.contains("10.0.0.24:4242"));
    assert!(
        stub.contains("LogChunk"),
        "missing capability mention: {stub}"
    );
    assert!(stub.contains("peer: 10.0.0.24:4242"));
}

#[test]
fn udp_probe_failed_names_step_and_count() {
    let out = DiagOutcome::UdpProbeFailed {
        peer: make_peer(),
        step: "recv_chunk",
        elapsed_ms: 1500,
        chunks_received: 3,
        error: "lost peer".into(),
    };
    let stub = out.stub_text();
    assert!(stub.contains("10.0.0.24:4242"));
    assert!(stub.contains("step: recv_chunk"));
    assert!(stub.contains("chunks_received: 3"));
}

#[test]
fn source_str_only_set_when_reachable() {
    assert_eq!(
        DiagOutcome::Captured {
            source: DiagSource::SetupCdc,
            text: String::new(),
            lost: 0,
        }
        .source_str(),
        Some("setup-cdc"),
    );
    assert_eq!(
        DiagOutcome::Empty {
            source: DiagSource::RunUdp { peer: make_peer() },
        }
        .source_str(),
        Some("run-udp"),
    );
    assert!(DiagOutcome::NoSetupPort.source_str().is_none());
    assert!(
        DiagOutcome::UdpUnsupported { peer: make_peer() }
            .source_str()
            .is_none()
    );
}

#[test]
fn failed_diag_selection_surfaces_udp_last_known_failure() {
    let selected = choose_failed_diag_outcome(
        DiagOutcome::NoSetupPort,
        DiagOutcome::VendorNotFound,
        DiagOutcome::UdpDiscoveryFailed {
            reason: "broadcast: no ack; unicast: no ack".into(),
        },
    );

    assert_eq!(selected.discriminant_str(), "udp_discovery_failed");
    let stub = selected.stub_text();
    assert!(
        stub.contains("run-mode UDP probe"),
        "wrong stub selected: {stub}"
    );
}

#[test]
fn failed_diag_selection_keeps_setup_probe_detail() {
    let selected = choose_failed_diag_outcome(
        DiagOutcome::SetupProbeFailed {
            port: "COM3".into(),
            step: "read",
            elapsed_ms: 3012,
            bytes_received: 0,
            rx_first_32_hex: "none".into(),
            error: "timed out".into(),
        },
        DiagOutcome::VendorNotFound,
        DiagOutcome::UdpDiscoveryFailed {
            reason: "no ack".into(),
        },
    );

    assert_eq!(selected.discriminant_str(), "setup_probe_failed");
    assert!(selected.stub_text().contains("port: COM3"));
}

#[test]
fn failed_diag_selection_distinguishes_no_known_run_mode_pico() {
    let selected = choose_failed_diag_outcome(
        DiagOutcome::NoSetupPort,
        DiagOutcome::VendorNotFound,
        DiagOutcome::NoLastPicoInConfig,
    );

    assert_eq!(selected.discriminant_str(), "no_last_pico_in_config");
}
