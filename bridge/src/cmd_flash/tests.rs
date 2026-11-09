use super::*;

#[test]
fn rp2040_accepts_rp2040_and_absolute_only() {
    let b = BootselBoard::Rp2040;
    assert!(b.accepts_family(FAMILY_RP2040));
    assert!(b.accepts_family(FAMILY_ABSOLUTE));
    assert!(!b.accepts_family(FAMILY_DATA));
    assert!(!b.accepts_family(FAMILY_RP2350_ARM_S));
    assert!(!b.accepts_family(FAMILY_RP2350_ARM_NS));
    assert!(!b.accepts_family(FAMILY_RP2350_RISCV));
    assert!(!b.accepts_family(0xDEAD_BEEF));
}

#[test]
fn rp2350_accepts_rp2350_variants_and_absolute_only() {
    let b = BootselBoard::Rp2350;
    assert!(b.accepts_family(FAMILY_RP2350_ARM_S));
    assert!(b.accepts_family(FAMILY_RP2350_ARM_NS));
    assert!(b.accepts_family(FAMILY_RP2350_RISCV));
    assert!(b.accepts_family(FAMILY_ABSOLUTE));
    assert!(!b.accepts_family(FAMILY_RP2040));
    assert!(!b.accepts_family(FAMILY_DATA));
    assert!(!b.accepts_family(0xDEAD_BEEF));
}

#[test]
fn family_name_covers_known_ids() {
    assert_eq!(family_name(FAMILY_RP2040), "rp2040");
    assert_eq!(family_name(FAMILY_ABSOLUTE), "absolute");
    assert_eq!(family_name(FAMILY_DATA), "data");
    assert_eq!(family_name(FAMILY_RP2350_ARM_S), "rp2350-arm-s");
    assert_eq!(family_name(FAMILY_RP2350_ARM_NS), "rp2350-arm-ns");
    assert_eq!(family_name(FAMILY_RP2350_RISCV), "rp2350-riscv");
    assert_eq!(family_name(0xDEAD_BEEF), "unknown");
}

#[test]
fn info_uf2_board_id_matches_known_boards() {
    assert_eq!(
        BootselBoard::from_info_uf2_board_id("RPI-RP2"),
        Some(BootselBoard::Rp2040)
    );
    assert_eq!(
        BootselBoard::from_info_uf2_board_id("RP2350"),
        Some(BootselBoard::Rp2350)
    );
    assert_eq!(
        BootselBoard::from_info_uf2_board_id(" RPI-RP2 \r\n"),
        Some(BootselBoard::Rp2040)
    );
    assert_eq!(
        BootselBoard::from_info_uf2_board_id("CUSTOMVENDOR-WIDGET"),
        None
    );
    assert_eq!(BootselBoard::from_info_uf2_board_id(""), None);
}
