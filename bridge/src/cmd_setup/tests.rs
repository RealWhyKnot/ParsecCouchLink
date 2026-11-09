use super::SETUP_STAGE_NAMES;
use super::setup_cdc::setup_recovery_candidate_uids;
use crate::{cmd_run::PicoTarget, protocol};
use std::collections::HashSet;
use std::net::{IpAddr, Ipv4Addr, SocketAddr};

#[test]
fn first_run_setup_does_not_require_controller_input() {
    let joined = SETUP_STAGE_NAMES.join(" ").to_ascii_lowercase();
    assert_eq!(SETUP_STAGE_NAMES.len(), 6);
    assert!(!joined.contains("xinput"));
    assert!(!joined.contains("controller"));
    assert!(!joined.contains("smoke"));
}

#[test]
fn setup_recovery_targets_only_new_run_mode_picos() {
    let old = pico_target(0x1111_1111);
    let flashed = pico_target(0x2222_2222);
    let mut baseline = HashSet::new();
    baseline.insert(old.info.unique_id_short);

    let candidates = setup_recovery_candidate_uids(&[old, flashed], &baseline);

    assert_eq!(candidates, vec![0x2222_2222]);
}

#[test]
fn setup_recovery_does_not_target_baseline_picos() {
    let old = pico_target(0x1111_1111);
    let mut baseline = HashSet::new();
    baseline.insert(old.info.unique_id_short);

    let candidates = setup_recovery_candidate_uids(&[old], &baseline);

    assert_eq!(candidates, <[u32; 0]>::default());
}

fn pico_target(uid: u32) -> PicoTarget {
    PicoTarget {
        peer: SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), 4242),
        info: protocol::AckInfo {
            proto_version: protocol::PROTO_VERSION,
            fw_major: 26,
            fw_minor: 6,
            fw_patch: 16,
            uptime_seconds: 1,
            unique_id_short: uid,
            board_type: protocol::BOARD_PICO_2_W,
            full_version: None,
        },
        persona: protocol::Persona::Ps4,
        ack_flags: protocol::ACK_FLAG_USB_DIAG_SUPPORTED,
    }
}
