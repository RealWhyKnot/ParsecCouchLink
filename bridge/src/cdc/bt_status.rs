use anyhow::{Result, bail};

use super::{
    BT_STATUS_FIXED_LEN, BT_STATUS_FLAG_CONNECTED, BT_STATUS_FLAG_SEND_REQUESTED,
    BT_STATUS_FLAG_STARTED, BT_STATUS_V1_FIXED_LEN, BT_STATUS_V1_VERSION, BT_STATUS_V2_FIXED_LEN,
    BT_STATUS_V2_VERSION, BT_STATUS_V3_FIXED_LEN, BT_STATUS_V3_VERSION, BT_STATUS_V4_FIXED_LEN,
    BT_STATUS_V4_VERSION, BT_STATUS_VERSION,
};
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) struct BtStatus {
    pub status_version: u8,
    pub decoded_status_version: u8,
    pub flags: u8,
    pub target: u8,
    pub last_status: u8,
    pub report_len: u8,
    pub cid: u16,
    pub init_count: u32,
    pub ready_count: u32,
    pub open_count: u32,
    pub close_count: u32,
    pub can_send_count: u32,
    pub report_build_count: u32,
    pub report_send_count: u32,
    pub send_request_count: u32,
    pub last_event_ms: u32,
    pub last_send_ms: u32,
    pub get_report_count: u32,
    pub get_report_success_count: u32,
    pub get_report_unsupported_count: u32,
    pub set_report_count: u32,
    pub set_report_accepted_count: u32,
    pub set_report_unsupported_count: u32,
    pub out_report_count: u32,
    pub out_report_accepted_count: u32,
    pub out_report_unsupported_count: u32,
    pub last_get_report_id: u8,
    pub last_get_report_type: u8,
    pub last_set_report_id: u8,
    pub last_set_report_type: u8,
    pub last_out_report_id: u8,
    pub last_out_report_type: u8,
    pub last_get_report_len: u16,
    pub last_set_report_len: u16,
    pub last_out_report_len: u16,
    pub pin_code_request_count: u32,
    pub pin_code_response_count: u32,
    pub user_confirmation_request_count: u32,
    pub user_confirmation_response_count: u32,
    pub simple_pairing_complete_count: u32,
    pub authentication_complete_count: u32,
    pub link_key_notification_count: u32,
    pub encryption_change_count: u32,
    pub disconnection_complete_count: u32,
    pub hid_open_failed_count: u32,
    pub last_security_event_ms: u32,
    pub last_simple_pairing_status: u8,
    pub last_authentication_status: u8,
    pub last_encryption_status: u8,
    pub last_encryption_enabled: u8,
    pub last_disconnection_reason: u8,
    pub last_hid_open_status: u8,
    pub reconnect_state: u8,
    pub reconnect_cycle_attempts: u8,
    pub last_reconnect_status: u8,
    pub last_reconnect_reason: u8,
    pub reconnect_schedule_count: u32,
    pub reconnect_attempt_count: u32,
    pub reconnect_success_count: u32,
    pub reconnect_failed_count: u32,
    pub reconnect_blocked_count: u32,
    pub last_reconnect_ms: u32,
    pub connection_complete_count: u32,
    pub last_connection_complete_status: u8,
    pub last_connection_complete_link_type: u8,
    pub last_connection_complete_ms: u32,
    pub incoming_l2cap_connection_count: u32,
    pub incoming_l2cap_hid_control_count: u32,
    pub incoming_l2cap_hid_interrupt_count: u32,
    pub last_incoming_l2cap_psm: u16,
    pub last_incoming_l2cap_local_cid: u16,
    pub last_incoming_l2cap_ms: u32,
    pub bt_cdc_state_count: u32,
    pub bt_cdc_heartbeat_count: u32,
    pub bt_cdc_bad_length_count: u32,
    pub bt_cdc_rejected_count: u32,
    pub bt_cdc_last_frame_ms: u32,
    pub bt_cdc_last_state_ms: u32,
    pub bt_cdc_last_heartbeat_ms: u32,
    pub bt_cdc_last_seq: u8,
    pub bt_cdc_last_command: u8,
    pub bt_cdc_last_flags: u8,
    pub local_name: String,
}

impl BtStatus {
    pub(crate) fn started(&self) -> bool {
        self.flags & BT_STATUS_FLAG_STARTED != 0
    }

    pub(crate) fn connected(&self) -> bool {
        self.flags & BT_STATUS_FLAG_CONNECTED != 0
    }

    pub(crate) fn send_requested(&self) -> bool {
        self.flags & BT_STATUS_FLAG_SEND_REQUESTED != 0
    }

    pub(crate) fn pairing_security_contact_seen(&self) -> bool {
        self.pin_code_request_count > 0
            || self.pin_code_response_count > 0
            || self.user_confirmation_request_count > 0
            || self.user_confirmation_response_count > 0
            || self.simple_pairing_complete_count > 0
            || self.authentication_complete_count > 0
            || self.link_key_notification_count > 0
            || self.encryption_change_count > 0
            || self.disconnection_complete_count > 0
            || self.last_security_event_ms > 0
    }

    pub(crate) fn newer_status_version(&self) -> bool {
        self.status_version > BT_STATUS_VERSION
    }

    #[cfg(test)]
    pub(crate) fn reconnect_peer_known(&self) -> bool {
        self.reconnect_state & 0x01 != 0
    }

    pub(crate) fn reconnect_pending(&self) -> bool {
        self.reconnect_state & 0x02 != 0
    }

    pub(crate) fn reconnect_in_progress(&self) -> bool {
        self.reconnect_state & 0x04 != 0
    }

    #[cfg(test)]
    pub(crate) fn reconnect_maxed(&self) -> bool {
        self.reconnect_state & 0x08 != 0
    }

    pub(crate) fn reconnect_activity_seen(&self) -> bool {
        self.reconnect_schedule_count > 0
            || self.reconnect_attempt_count > 0
            || self.reconnect_success_count > 0
            || self.reconnect_failed_count > 0
            || self.reconnect_blocked_count > 0
            || self.last_reconnect_ms > 0
    }

    pub(crate) fn incoming_hid_l2cap_seen(&self) -> bool {
        self.incoming_l2cap_connection_count > 0
            || self.incoming_l2cap_hid_control_count > 0
            || self.incoming_l2cap_hid_interrupt_count > 0
            || self.last_incoming_l2cap_psm != 0
    }

    pub(crate) fn bt_cdc_counters_captured(&self) -> bool {
        self.decoded_status_version >= BT_STATUS_VERSION
    }

    pub(crate) fn bt_cdc_input_seen(&self) -> bool {
        self.bt_cdc_counters_captured()
            && (self.bt_cdc_state_count > 0 || self.bt_cdc_heartbeat_count > 0)
    }
}

fn read_u16_le(payload: &[u8], offset: usize) -> u16 {
    u16::from_le_bytes([payload[offset], payload[offset + 1]])
}

fn read_u32_le(payload: &[u8], offset: usize) -> u32 {
    u32::from_le_bytes([
        payload[offset],
        payload[offset + 1],
        payload[offset + 2],
        payload[offset + 3],
    ])
}

struct BtStatusLayout {
    version: u8,
    name_start: usize,
    name_end: usize,
}

fn bt_status_layout(payload: &[u8]) -> Result<BtStatusLayout> {
    if payload.is_empty() {
        bail!("BT_STATUS response truncated ({} bytes)", payload.len());
    }
    let version = payload[0];
    let fixed_len = match version {
        BT_STATUS_V1_VERSION => BT_STATUS_V1_FIXED_LEN,
        BT_STATUS_V2_VERSION => BT_STATUS_V2_FIXED_LEN,
        BT_STATUS_V3_VERSION => BT_STATUS_V3_FIXED_LEN,
        BT_STATUS_V4_VERSION => BT_STATUS_V4_FIXED_LEN,
        BT_STATUS_VERSION => BT_STATUS_FIXED_LEN,
        newer if newer > BT_STATUS_VERSION => BT_STATUS_FIXED_LEN,
        _ => bail!(
            "BT_STATUS version mismatch (got {}, want {}, {}, {}, {}, or {})",
            payload[0],
            BT_STATUS_V1_VERSION,
            BT_STATUS_V2_VERSION,
            BT_STATUS_V3_VERSION,
            BT_STATUS_V4_VERSION,
            BT_STATUS_VERSION
        ),
    };
    if payload.len() < fixed_len {
        bail!("BT_STATUS response truncated ({} bytes)", payload.len());
    }
    let decode_name = version <= BT_STATUS_VERSION;
    let name_len_offset = match version {
        BT_STATUS_V1_VERSION => BT_STATUS_V1_FIXED_LEN - 1,
        BT_STATUS_V2_VERSION => BT_STATUS_V2_FIXED_LEN - 1,
        BT_STATUS_V3_VERSION => BT_STATUS_V3_FIXED_LEN - 1,
        BT_STATUS_V4_VERSION => BT_STATUS_V4_FIXED_LEN - 1,
        BT_STATUS_VERSION => BT_STATUS_FIXED_LEN - 1,
        _ => payload.len(),
    };
    let name_start = fixed_len;
    let name_len = if decode_name {
        usize::from(payload[name_len_offset])
    } else {
        0
    };
    let need = if decode_name {
        name_start + name_len
    } else {
        name_start
    };
    if payload.len() < need {
        bail!(
            "BT_STATUS local name truncated (need {need} bytes, have {})",
            payload.len()
        );
    }
    Ok(BtStatusLayout {
        version,
        name_start,
        name_end: need,
    })
}

fn read_report_counters(status: &mut BtStatus, payload: &[u8]) {
    status.get_report_count = read_u32_le(payload, 48);
    status.get_report_success_count = read_u32_le(payload, 52);
    status.get_report_unsupported_count = read_u32_le(payload, 56);
    status.set_report_count = read_u32_le(payload, 60);
    status.set_report_accepted_count = read_u32_le(payload, 64);
    status.set_report_unsupported_count = read_u32_le(payload, 68);
    status.out_report_count = read_u32_le(payload, 72);
    status.out_report_accepted_count = read_u32_le(payload, 76);
    status.out_report_unsupported_count = read_u32_le(payload, 80);
    status.last_get_report_id = payload[84];
    status.last_get_report_type = payload[85];
    status.last_set_report_id = payload[86];
    status.last_set_report_type = payload[87];
    status.last_out_report_id = payload[88];
    status.last_out_report_type = payload[89];
    status.last_get_report_len = read_u16_le(payload, 92);
    status.last_set_report_len = read_u16_le(payload, 94);
    status.last_out_report_len = read_u16_le(payload, 96);
}

fn read_security_counters(status: &mut BtStatus, payload: &[u8]) {
    status.pin_code_request_count = read_u32_le(payload, 98);
    status.pin_code_response_count = read_u32_le(payload, 102);
    status.user_confirmation_request_count = read_u32_le(payload, 106);
    status.user_confirmation_response_count = read_u32_le(payload, 110);
    status.simple_pairing_complete_count = read_u32_le(payload, 114);
    status.authentication_complete_count = read_u32_le(payload, 118);
    status.link_key_notification_count = read_u32_le(payload, 122);
    status.encryption_change_count = read_u32_le(payload, 126);
    status.disconnection_complete_count = read_u32_le(payload, 130);
    status.hid_open_failed_count = read_u32_le(payload, 134);
    status.last_security_event_ms = read_u32_le(payload, 138);
    status.last_simple_pairing_status = payload[142];
    status.last_authentication_status = payload[143];
    status.last_encryption_status = payload[144];
    status.last_encryption_enabled = payload[145];
    status.last_disconnection_reason = payload[146];
    status.last_hid_open_status = payload[147];
}

fn read_reconnect_counters(status: &mut BtStatus, payload: &[u8]) {
    status.reconnect_state = payload[152];
    status.reconnect_cycle_attempts = payload[153];
    status.last_reconnect_status = payload[154];
    status.last_reconnect_reason = payload[155];
    status.reconnect_schedule_count = read_u32_le(payload, 156);
    status.reconnect_attempt_count = read_u32_le(payload, 160);
    status.reconnect_success_count = read_u32_le(payload, 164);
    status.reconnect_failed_count = read_u32_le(payload, 168);
    status.reconnect_blocked_count = read_u32_le(payload, 172);
    status.last_reconnect_ms = read_u32_le(payload, 176);
    status.connection_complete_count = read_u32_le(payload, 180);
    status.last_connection_complete_status = payload[184];
    status.last_connection_complete_link_type = payload[185];
    status.last_connection_complete_ms = read_u32_le(payload, 188);
    status.incoming_l2cap_connection_count = read_u32_le(payload, 192);
    status.incoming_l2cap_hid_control_count = read_u32_le(payload, 196);
    status.incoming_l2cap_hid_interrupt_count = read_u32_le(payload, 200);
    status.last_incoming_l2cap_psm = read_u16_le(payload, 204);
    status.last_incoming_l2cap_local_cid = read_u16_le(payload, 206);
    status.last_incoming_l2cap_ms = read_u32_le(payload, 208);
}

fn read_cdc_counters(status: &mut BtStatus, payload: &[u8]) {
    status.bt_cdc_state_count = read_u32_le(payload, 212);
    status.bt_cdc_heartbeat_count = read_u32_le(payload, 216);
    status.bt_cdc_bad_length_count = read_u32_le(payload, 220);
    status.bt_cdc_rejected_count = read_u32_le(payload, 224);
    status.bt_cdc_last_frame_ms = read_u32_le(payload, 228);
    status.bt_cdc_last_state_ms = read_u32_le(payload, 232);
    status.bt_cdc_last_heartbeat_ms = read_u32_le(payload, 236);
    status.bt_cdc_last_seq = payload[240];
    status.bt_cdc_last_command = payload[241];
    status.bt_cdc_last_flags = payload[242];
}

pub(crate) fn decode_bt_status_payload(payload: &[u8]) -> Result<BtStatus> {
    let layout = bt_status_layout(payload)?;
    let version = layout.version;
    let has_v2 = version == BT_STATUS_V2_VERSION
        || version == BT_STATUS_V3_VERSION
        || version == BT_STATUS_V4_VERSION
        || version >= BT_STATUS_VERSION;
    let has_v3 = version == BT_STATUS_V3_VERSION
        || version == BT_STATUS_V4_VERSION
        || version >= BT_STATUS_VERSION;
    let has_v4 = version == BT_STATUS_V4_VERSION || version >= BT_STATUS_VERSION;
    let has_v5 = version >= BT_STATUS_VERSION;
    let mut status = BtStatus {
        status_version: version,
        decoded_status_version: if version > BT_STATUS_VERSION {
            BT_STATUS_VERSION
        } else {
            version
        },
        flags: payload[1],
        target: payload[2],
        last_status: payload[3],
        report_len: payload[4],
        cid: read_u16_le(payload, 6),
        init_count: read_u32_le(payload, 8),
        ready_count: read_u32_le(payload, 12),
        open_count: read_u32_le(payload, 16),
        close_count: read_u32_le(payload, 20),
        can_send_count: read_u32_le(payload, 24),
        report_build_count: read_u32_le(payload, 28),
        report_send_count: read_u32_le(payload, 32),
        send_request_count: read_u32_le(payload, 36),
        last_event_ms: read_u32_le(payload, 40),
        last_send_ms: read_u32_le(payload, 44),
        local_name: String::from_utf8_lossy(&payload[layout.name_start..layout.name_end])
            .into_owned(),
        ..BtStatus::default()
    };
    if has_v2 {
        read_report_counters(&mut status, payload);
    }
    if has_v3 {
        read_security_counters(&mut status, payload);
    }
    if has_v4 {
        read_reconnect_counters(&mut status, payload);
    }
    if has_v5 {
        read_cdc_counters(&mut status, payload);
    }
    Ok(status)
}
