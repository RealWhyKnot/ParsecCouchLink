use anyhow::{bail, Result};

use super::{
    BT_STATUS_FIXED_LEN, BT_STATUS_FLAG_CONNECTED, BT_STATUS_FLAG_SEND_REQUESTED,
    BT_STATUS_FLAG_STARTED, BT_STATUS_V1_FIXED_LEN, BT_STATUS_V1_VERSION, BT_STATUS_V2_FIXED_LEN,
    BT_STATUS_V2_VERSION, BT_STATUS_V3_FIXED_LEN, BT_STATUS_V3_VERSION, BT_STATUS_V4_FIXED_LEN,
    BT_STATUS_V4_VERSION, BT_STATUS_VERSION,
};
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BtStatus {
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
    pub fn started(&self) -> bool {
        self.flags & BT_STATUS_FLAG_STARTED != 0
    }

    pub fn connected(&self) -> bool {
        self.flags & BT_STATUS_FLAG_CONNECTED != 0
    }

    pub fn send_requested(&self) -> bool {
        self.flags & BT_STATUS_FLAG_SEND_REQUESTED != 0
    }

    pub fn pairing_security_contact_seen(&self) -> bool {
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

    pub fn newer_status_version(&self) -> bool {
        self.status_version > BT_STATUS_VERSION
    }

    pub fn reconnect_peer_known(&self) -> bool {
        self.reconnect_state & 0x01 != 0
    }

    pub fn reconnect_pending(&self) -> bool {
        self.reconnect_state & 0x02 != 0
    }

    pub fn reconnect_in_progress(&self) -> bool {
        self.reconnect_state & 0x04 != 0
    }

    pub fn reconnect_maxed(&self) -> bool {
        self.reconnect_state & 0x08 != 0
    }

    pub fn reconnect_activity_seen(&self) -> bool {
        self.reconnect_schedule_count > 0
            || self.reconnect_attempt_count > 0
            || self.reconnect_success_count > 0
            || self.reconnect_failed_count > 0
            || self.reconnect_blocked_count > 0
            || self.last_reconnect_ms > 0
    }

    pub fn incoming_hid_l2cap_seen(&self) -> bool {
        self.incoming_l2cap_connection_count > 0
            || self.incoming_l2cap_hid_control_count > 0
            || self.incoming_l2cap_hid_interrupt_count > 0
            || self.last_incoming_l2cap_psm != 0
    }

    pub fn bt_cdc_input_seen(&self) -> bool {
        self.bt_cdc_state_count > 0 || self.bt_cdc_heartbeat_count > 0
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

pub fn decode_bt_status_payload(payload: &[u8]) -> Result<BtStatus> {
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
        payload[name_len_offset] as usize
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
    let has_v2 = version == BT_STATUS_V2_VERSION
        || version == BT_STATUS_V3_VERSION
        || version == BT_STATUS_V4_VERSION
        || version >= BT_STATUS_VERSION;
    let has_v3 = version == BT_STATUS_V3_VERSION
        || version == BT_STATUS_V4_VERSION
        || version >= BT_STATUS_VERSION;
    let has_v4 = version == BT_STATUS_V4_VERSION || version >= BT_STATUS_VERSION;
    let has_v5 = version >= BT_STATUS_VERSION;
    Ok(BtStatus {
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
        get_report_count: if has_v2 { read_u32_le(payload, 48) } else { 0 },
        get_report_success_count: if has_v2 { read_u32_le(payload, 52) } else { 0 },
        get_report_unsupported_count: if has_v2 { read_u32_le(payload, 56) } else { 0 },
        set_report_count: if has_v2 { read_u32_le(payload, 60) } else { 0 },
        set_report_accepted_count: if has_v2 { read_u32_le(payload, 64) } else { 0 },
        set_report_unsupported_count: if has_v2 { read_u32_le(payload, 68) } else { 0 },
        out_report_count: if has_v2 { read_u32_le(payload, 72) } else { 0 },
        out_report_accepted_count: if has_v2 { read_u32_le(payload, 76) } else { 0 },
        out_report_unsupported_count: if has_v2 { read_u32_le(payload, 80) } else { 0 },
        last_get_report_id: if has_v2 { payload[84] } else { 0 },
        last_get_report_type: if has_v2 { payload[85] } else { 0 },
        last_set_report_id: if has_v2 { payload[86] } else { 0 },
        last_set_report_type: if has_v2 { payload[87] } else { 0 },
        last_out_report_id: if has_v2 { payload[88] } else { 0 },
        last_out_report_type: if has_v2 { payload[89] } else { 0 },
        last_get_report_len: if has_v2 { read_u16_le(payload, 92) } else { 0 },
        last_set_report_len: if has_v2 { read_u16_le(payload, 94) } else { 0 },
        last_out_report_len: if has_v2 { read_u16_le(payload, 96) } else { 0 },
        pin_code_request_count: if has_v3 { read_u32_le(payload, 98) } else { 0 },
        pin_code_response_count: if has_v3 { read_u32_le(payload, 102) } else { 0 },
        user_confirmation_request_count: if has_v3 { read_u32_le(payload, 106) } else { 0 },
        user_confirmation_response_count: if has_v3 { read_u32_le(payload, 110) } else { 0 },
        simple_pairing_complete_count: if has_v3 { read_u32_le(payload, 114) } else { 0 },
        authentication_complete_count: if has_v3 { read_u32_le(payload, 118) } else { 0 },
        link_key_notification_count: if has_v3 { read_u32_le(payload, 122) } else { 0 },
        encryption_change_count: if has_v3 { read_u32_le(payload, 126) } else { 0 },
        disconnection_complete_count: if has_v3 { read_u32_le(payload, 130) } else { 0 },
        hid_open_failed_count: if has_v3 { read_u32_le(payload, 134) } else { 0 },
        last_security_event_ms: if has_v3 { read_u32_le(payload, 138) } else { 0 },
        last_simple_pairing_status: if has_v3 { payload[142] } else { 0 },
        last_authentication_status: if has_v3 { payload[143] } else { 0 },
        last_encryption_status: if has_v3 { payload[144] } else { 0 },
        last_encryption_enabled: if has_v3 { payload[145] } else { 0 },
        last_disconnection_reason: if has_v3 { payload[146] } else { 0 },
        last_hid_open_status: if has_v3 { payload[147] } else { 0 },
        reconnect_state: if has_v4 { payload[152] } else { 0 },
        reconnect_cycle_attempts: if has_v4 { payload[153] } else { 0 },
        last_reconnect_status: if has_v4 { payload[154] } else { 0 },
        last_reconnect_reason: if has_v4 { payload[155] } else { 0 },
        reconnect_schedule_count: if has_v4 { read_u32_le(payload, 156) } else { 0 },
        reconnect_attempt_count: if has_v4 { read_u32_le(payload, 160) } else { 0 },
        reconnect_success_count: if has_v4 { read_u32_le(payload, 164) } else { 0 },
        reconnect_failed_count: if has_v4 { read_u32_le(payload, 168) } else { 0 },
        reconnect_blocked_count: if has_v4 { read_u32_le(payload, 172) } else { 0 },
        last_reconnect_ms: if has_v4 { read_u32_le(payload, 176) } else { 0 },
        connection_complete_count: if has_v4 { read_u32_le(payload, 180) } else { 0 },
        last_connection_complete_status: if has_v4 { payload[184] } else { 0 },
        last_connection_complete_link_type: if has_v4 { payload[185] } else { 0 },
        last_connection_complete_ms: if has_v4 { read_u32_le(payload, 188) } else { 0 },
        incoming_l2cap_connection_count: if has_v4 { read_u32_le(payload, 192) } else { 0 },
        incoming_l2cap_hid_control_count: if has_v4 { read_u32_le(payload, 196) } else { 0 },
        incoming_l2cap_hid_interrupt_count: if has_v4 { read_u32_le(payload, 200) } else { 0 },
        last_incoming_l2cap_psm: if has_v4 { read_u16_le(payload, 204) } else { 0 },
        last_incoming_l2cap_local_cid: if has_v4 { read_u16_le(payload, 206) } else { 0 },
        last_incoming_l2cap_ms: if has_v4 { read_u32_le(payload, 208) } else { 0 },
        bt_cdc_state_count: if has_v5 { read_u32_le(payload, 212) } else { 0 },
        bt_cdc_heartbeat_count: if has_v5 { read_u32_le(payload, 216) } else { 0 },
        bt_cdc_bad_length_count: if has_v5 { read_u32_le(payload, 220) } else { 0 },
        bt_cdc_rejected_count: if has_v5 { read_u32_le(payload, 224) } else { 0 },
        bt_cdc_last_frame_ms: if has_v5 { read_u32_le(payload, 228) } else { 0 },
        bt_cdc_last_state_ms: if has_v5 { read_u32_le(payload, 232) } else { 0 },
        bt_cdc_last_heartbeat_ms: if has_v5 { read_u32_le(payload, 236) } else { 0 },
        bt_cdc_last_seq: if has_v5 { payload[240] } else { 0 },
        bt_cdc_last_command: if has_v5 { payload[241] } else { 0 },
        bt_cdc_last_flags: if has_v5 { payload[242] } else { 0 },
        local_name: String::from_utf8_lossy(&payload[name_start..need]).into_owned(),
    })
}
