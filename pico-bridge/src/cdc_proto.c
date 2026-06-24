#include "cdc_proto.h"

#include <string.h>

static void put_u16_le(uint8_t *out, uint16_t value) {
    out[0] = (uint8_t)(value & 0xFFu);
    out[1] = (uint8_t)((value >> 8) & 0xFFu);
}

static void put_u32_le(uint8_t *out, uint32_t value) {
    out[0] = (uint8_t)(value & 0xFFu);
    out[1] = (uint8_t)((value >> 8) & 0xFFu);
    out[2] = (uint8_t)((value >> 16) & 0xFFu);
    out[3] = (uint8_t)((value >> 24) & 0xFFu);
}

uint16_t cdc_crc16(const uint8_t *data, size_t n) {
    uint16_t crc = 0xFFFF;
    for (size_t i = 0; i < n; i++) {
        crc ^= ((uint16_t)data[i]) << 8;
        for (int b = 0; b < 8; b++) {
            if (crc & 0x8000u)
                crc = (crc << 1) ^ 0x1021u;
            else
                crc = (crc << 1);
        }
    }
    return crc;
}

size_t cdc_build_bt_status_payload(const cdc_bt_status_view_t *status, uint8_t *out,
                                   size_t out_cap) {
    if (!status || !out || out_cap < CDC_BT_STATUS_FIXED_LEN)
        return 0;

    size_t name_len = 0;
    if (status->local_name) {
        name_len = strlen(status->local_name);
        if (name_len > CDC_BT_STATUS_MAX_NAME)
            name_len = CDC_BT_STATUS_MAX_NAME;
    }
    if (out_cap < CDC_BT_STATUS_FIXED_LEN + name_len)
        return 0;

    out[0] = CDC_BT_STATUS_VERSION;
    out[1] = status->flags;
    out[2] = status->target;
    out[3] = status->last_status;
    out[4] = status->report_len;
    out[5] = 0;
    put_u16_le(&out[6], status->cid);
    put_u32_le(&out[8], status->init_count);
    put_u32_le(&out[12], status->ready_count);
    put_u32_le(&out[16], status->open_count);
    put_u32_le(&out[20], status->close_count);
    put_u32_le(&out[24], status->can_send_count);
    put_u32_le(&out[28], status->report_build_count);
    put_u32_le(&out[32], status->report_send_count);
    put_u32_le(&out[36], status->send_request_count);
    put_u32_le(&out[40], status->last_event_ms);
    put_u32_le(&out[44], status->last_send_ms);
    put_u32_le(&out[48], status->get_report_count);
    put_u32_le(&out[52], status->get_report_success_count);
    put_u32_le(&out[56], status->get_report_unsupported_count);
    put_u32_le(&out[60], status->set_report_count);
    put_u32_le(&out[64], status->set_report_accepted_count);
    put_u32_le(&out[68], status->set_report_unsupported_count);
    put_u32_le(&out[72], status->out_report_count);
    put_u32_le(&out[76], status->out_report_accepted_count);
    put_u32_le(&out[80], status->out_report_unsupported_count);
    out[84] = status->last_get_report_id;
    out[85] = status->last_get_report_type;
    out[86] = status->last_set_report_id;
    out[87] = status->last_set_report_type;
    out[88] = status->last_out_report_id;
    out[89] = status->last_out_report_type;
    out[90] = 0;
    out[91] = 0;
    put_u16_le(&out[92], status->last_get_report_len);
    put_u16_le(&out[94], status->last_set_report_len);
    put_u16_le(&out[96], status->last_out_report_len);
    put_u32_le(&out[98], status->pin_code_request_count);
    put_u32_le(&out[102], status->pin_code_response_count);
    put_u32_le(&out[106], status->user_confirmation_request_count);
    put_u32_le(&out[110], status->user_confirmation_response_count);
    put_u32_le(&out[114], status->simple_pairing_complete_count);
    put_u32_le(&out[118], status->authentication_complete_count);
    put_u32_le(&out[122], status->link_key_notification_count);
    put_u32_le(&out[126], status->encryption_change_count);
    put_u32_le(&out[130], status->disconnection_complete_count);
    put_u32_le(&out[134], status->hid_open_failed_count);
    put_u32_le(&out[138], status->last_security_event_ms);
    out[142] = status->last_simple_pairing_status;
    out[143] = status->last_authentication_status;
    out[144] = status->last_encryption_status;
    out[145] = status->last_encryption_enabled;
    out[146] = status->last_disconnection_reason;
    out[147] = status->last_hid_open_status;
    out[148] = 0;
    out[149] = 0;
    out[150] = 0;
    out[151] = 0;
    out[152] = status->reconnect_state;
    out[153] = status->reconnect_cycle_attempts;
    out[154] = status->last_reconnect_status;
    out[155] = status->last_reconnect_reason;
    put_u32_le(&out[156], status->reconnect_schedule_count);
    put_u32_le(&out[160], status->reconnect_attempt_count);
    put_u32_le(&out[164], status->reconnect_success_count);
    put_u32_le(&out[168], status->reconnect_failed_count);
    put_u32_le(&out[172], status->reconnect_blocked_count);
    put_u32_le(&out[176], status->last_reconnect_ms);
    put_u32_le(&out[180], status->connection_complete_count);
    out[184] = status->last_connection_complete_status;
    out[185] = status->last_connection_complete_link_type;
    out[186] = 0;
    out[187] = 0;
    put_u32_le(&out[188], status->last_connection_complete_ms);
    put_u32_le(&out[192], status->incoming_l2cap_connection_count);
    put_u32_le(&out[196], status->incoming_l2cap_hid_control_count);
    put_u32_le(&out[200], status->incoming_l2cap_hid_interrupt_count);
    put_u16_le(&out[204], status->last_incoming_l2cap_psm);
    put_u16_le(&out[206], status->last_incoming_l2cap_local_cid);
    put_u32_le(&out[208], status->last_incoming_l2cap_ms);
    put_u32_le(&out[212], status->bt_cdc_state_count);
    put_u32_le(&out[216], status->bt_cdc_heartbeat_count);
    put_u32_le(&out[220], status->bt_cdc_bad_length_count);
    put_u32_le(&out[224], status->bt_cdc_rejected_count);
    put_u32_le(&out[228], status->bt_cdc_last_frame_ms);
    put_u32_le(&out[232], status->bt_cdc_last_state_ms);
    put_u32_le(&out[236], status->bt_cdc_last_heartbeat_ms);
    out[240] = status->bt_cdc_last_seq;
    out[241] = status->bt_cdc_last_command;
    out[242] = status->bt_cdc_last_flags;
    out[243] = 0;
    out[244] = (uint8_t)name_len;
    if (name_len)
        memcpy(&out[245], status->local_name, name_len);
    return CDC_BT_STATUS_FIXED_LEN + name_len;
}

size_t cdc_encode(uint8_t command, uint8_t seq, const uint8_t *payload, size_t payload_len,
                  uint8_t *out, size_t out_cap) {
    if (payload_len > CDC_MAX_PAYLOAD)
        return 0;
    size_t total = CDC_HEADER_LEN + payload_len + CDC_CRC_LEN;
    if (out_cap < total)
        return 0;

    out[0] = CDC_FRAME_MAGIC0;
    out[1] = CDC_FRAME_MAGIC1;
    out[2] = CDC_PROTO_VERSION;
    out[3] = command;
    out[4] = (uint8_t)(payload_len & 0xFF);
    out[5] = (uint8_t)((payload_len >> 8) & 0xFF);
    out[6] = seq;
    out[7] = 0;
    if (payload_len)
        memcpy(&out[CDC_HEADER_LEN], payload, payload_len);

    uint16_t crc = cdc_crc16(out, CDC_HEADER_LEN + payload_len);
    out[CDC_HEADER_LEN + payload_len] = (uint8_t)(crc & 0xFF);
    out[CDC_HEADER_LEN + payload_len + 1] = (uint8_t)((crc >> 8) & 0xFF);
    return total;
}

cdc_decode_status_t cdc_try_decode(const uint8_t *buf, size_t buf_len, cdc_frame_view_t *out,
                                   size_t *consumed) {
    *consumed = 0;
    if (buf_len < CDC_HEADER_LEN + CDC_CRC_LEN)
        return CDC_DECODE_NEED_MORE;
    if (buf[0] != CDC_FRAME_MAGIC0 || buf[1] != CDC_FRAME_MAGIC1)
        return CDC_DECODE_BAD_MAGIC;
    if (buf[2] != CDC_PROTO_VERSION)
        return CDC_DECODE_BAD_VERSION;
    size_t payload_len = (size_t)buf[4] | ((size_t)buf[5] << 8);
    if (payload_len > CDC_MAX_PAYLOAD)
        return CDC_DECODE_BAD_LENGTH;
    size_t total = CDC_HEADER_LEN + payload_len + CDC_CRC_LEN;
    if (buf_len < total)
        return CDC_DECODE_NEED_MORE;

    uint16_t expected = (uint16_t)buf[CDC_HEADER_LEN + payload_len] |
                        ((uint16_t)buf[CDC_HEADER_LEN + payload_len + 1] << 8);
    uint16_t actual = cdc_crc16(buf, CDC_HEADER_LEN + payload_len);
    if (expected != actual)
        return CDC_DECODE_BAD_CRC;

    out->command = buf[3];
    out->seq = buf[6];
    out->payload = (payload_len > 0) ? &buf[CDC_HEADER_LEN] : NULL;
    out->payload_len = payload_len;
    *consumed = total;
    return CDC_DECODE_OK;
}
