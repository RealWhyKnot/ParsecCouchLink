use crate::protocol;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum DiagStage {
    Silent,
    DescriptorsOnly,
    Mounted,
    Polling,
    PollingWithOut,
}

pub(crate) fn usb_diag(stage: DiagStage) -> protocol::UsbDiag {
    let desc = stage != DiagStage::Silent;
    let mounted = matches!(
        stage,
        DiagStage::Mounted | DiagStage::Polling | DiagStage::PollingWithOut
    );
    let sent = matches!(stage, DiagStage::Polling | DiagStage::PollingWithOut);
    let out = stage == DiagStage::PollingWithOut;
    protocol::UsbDiag {
        seq: 1,
        flags: 0,
        version: protocol::USB_DIAG_VERSION,
        usb_flags: if mounted {
            protocol::USB_DIAG_FLAG_MOUNTED
        } else {
            0
        },
        activity_flags: (if sent {
            protocol::USB_DIAG_ACTIVITY_SENT
        } else {
            0
        }) | (if out {
            protocol::USB_DIAG_ACTIVITY_OUT
        } else {
            0
        }),
        last_out_len: 0,
        now_ms: 10_000,
        last_bridge_packet_ms: 0,
        mount_count: u32::from(mounted),
        umount_count: 0,
        suspend_count: 0,
        resume_count: 0,
        device_desc_count: u32::from(desc),
        config_desc_count: u32::from(desc),
        xinput_in_queued_count: u32::from(sent),
        xinput_in_sent_count: u32::from(sent),
        xinput_out_count: u32::from(out),
        xinput_in_blocked_not_mounted_count: 0,
        xinput_in_blocked_not_ready_count: 0,
        xinput_in_blocked_short_write_count: 0,
        xinput_in_idle_suppressed_count: 0,
        last_mount_ms: 9000,
        last_umount_ms: 0,
        last_in_queued_ms: 0,
        last_in_sent_ms: if sent { 9500 } else { 0 },
        last_out_ms: if out { 9600 } else { 0 },
        last_in_blocked_ms: 0,
        last_in_blocked_reason: 0,
        last_in_blocked_want: 0,
        last_in_blocked_got: 0,
        last_out_byte0: 0,
        last_out_byte1: 0,
    }
}
