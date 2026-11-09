use crate::protocol::*;

pub(super) fn make_chunk(idx: u8, flags: u8, total: u8, lost: u32, payload: &[u8]) -> LogChunk {
    LogChunk {
        chunk_index: idx,
        flags,
        total_chunks: total,
        lost_bytes: lost,
        payload: payload.to_vec(),
    }
}

#[test]
fn log_chunk_roundtrip_empty_payload() {
    let c = make_chunk(0, LOG_CHUNK_FLAG_LAST, 1, 0, &[]);
    let buf = c.encode();
    assert_eq!(buf.len(), LOG_CHUNK_HEADER_LEN + 2);
    let back = LogChunk::decode(&buf).unwrap();
    assert_eq!(back, c);
    assert!(back.is_last());
}

#[test]
fn log_chunk_roundtrip_max_payload() {
    let payload: Vec<u8> = (0..=u8::MAX).cycle().take(LOG_CHUNK_MAX_PAYLOAD).collect();
    assert_eq!(payload.len(), LOG_CHUNK_MAX_PAYLOAD);
    let c = make_chunk(5, 0, 16, 1234, &payload);
    let buf = c.encode();
    assert_eq!(buf.len(), LOG_CHUNK_HEADER_LEN + LOG_CHUNK_MAX_PAYLOAD + 2);
    let back = LogChunk::decode(&buf).unwrap();
    assert_eq!(back, c);
    assert!(!back.is_last());
    assert_eq!(back.lost_bytes, 1234);
}

#[test]
fn log_chunk_last_flag_decoded() {
    let c = make_chunk(15, LOG_CHUNK_FLAG_LAST, 16, 0, b"final-chunk");
    let buf = c.encode();
    let back = LogChunk::decode(&buf).unwrap();
    assert!(back.is_last());
}

#[test]
fn log_chunk_bad_crc_rejected() {
    let c = make_chunk(0, 0, 1, 0, b"hello");
    let mut buf = c.encode();
    let last = buf.len() - 1;
    buf[last] ^= 0xFF;
    match LogChunk::decode(&buf) {
        Err(LogChunkDecodeError::BadCrc { .. }) => (),
        other => panic!("expected BadCrc, got {other:?}"),
    }
}

#[test]
fn log_chunk_wrong_magic_rejected() {
    let c = make_chunk(0, 0, 1, 0, b"hi");
    let mut buf = c.encode();
    buf[0] = 0x00;
    match LogChunk::decode(&buf) {
        Err(LogChunkDecodeError::WrongMagic) => (),
        other => panic!("expected WrongMagic, got {other:?}"),
    }
}

#[test]
fn log_chunk_wrong_type_rejected() {
    let c = make_chunk(0, 0, 1, 0, b"hi");
    let mut buf = c.encode();
    buf[1] = TYPE_ACK;
    match LogChunk::decode(&buf) {
        Err(LogChunkDecodeError::WrongType(t)) if t == TYPE_ACK => (),
        other => panic!("expected WrongType(TYPE_ACK), got {other:?}"),
    }
}

#[test]
fn log_chunk_length_mismatch_rejected() {
    let c = make_chunk(0, 0, 1, 0, b"hi");
    let mut buf = c.encode();
    buf[5] = 99;
    match LogChunk::decode(&buf) {
        Err(LogChunkDecodeError::LengthMismatch { .. }) => (),
        other => panic!("expected LengthMismatch, got {other:?}"),
    }
}
