//! mainline-cursor-v1 encode/decode. See GRC-006 §2.7.
//!
//! Wire format (before base64url):
//!   chainIdShort  (4 bytes, big-endian)
//!   libNum        (8 bytes, big-endian u64)
//!   libHash       (32 bytes)
//!   headNum       (8 bytes, big-endian u64)
//!   headHash      (32 bytes)
//!   forkStepsSeen (varint, unsigned LEB128)
//!
//! Total minimum size: 4 + 8 + 32 + 8 + 32 + 1 = 85 bytes.
//!
//! The cursor is portable across operators because every field is globally
//! addressable chain state. Operators that cannot resume from a given cursor
//! (because their ForkDB is too shallow) must return
//! `MAINLINE_CURSOR_UNRESUMABLE` per §2.7.

use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine as _};
use thiserror::Error;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MainlineCursor {
    pub chain_id_short: [u8; 4],
    pub lib_num: u64,
    pub lib_hash: [u8; 32],
    pub head_num: u64,
    pub head_hash: [u8; 32],
    pub fork_steps_seen: u64,
}

#[derive(Error, Debug, PartialEq, Eq)]
pub enum CursorError {
    #[error("invalid base64url")]
    InvalidBase64,
    #[error("truncated cursor (expected at least {expected} bytes, got {actual})")]
    Truncated { expected: usize, actual: usize },
    #[error("trailing bytes after cursor")]
    Trailing,
    #[error("varint overflow")]
    VarintOverflow,
}

fn write_varint(out: &mut Vec<u8>, mut v: u64) {
    while v >= 0x80 {
        out.push(((v as u8) & 0x7f) | 0x80);
        v >>= 7;
    }
    out.push(v as u8);
}

fn read_varint(buf: &[u8]) -> Result<(u64, usize), CursorError> {
    let mut result: u64 = 0;
    let mut shift: u32 = 0;
    for (i, b) in buf.iter().enumerate() {
        if shift >= 64 {
            return Err(CursorError::VarintOverflow);
        }
        result |= ((b & 0x7f) as u64) << shift;
        if b & 0x80 == 0 {
            return Ok((result, i + 1));
        }
        shift += 7;
    }
    Err(CursorError::Truncated { expected: buf.len() + 1, actual: buf.len() })
}

pub fn encode(cursor: &MainlineCursor) -> String {
    let mut buf = Vec::with_capacity(96);
    buf.extend_from_slice(&cursor.chain_id_short);
    buf.extend_from_slice(&cursor.lib_num.to_be_bytes());
    buf.extend_from_slice(&cursor.lib_hash);
    buf.extend_from_slice(&cursor.head_num.to_be_bytes());
    buf.extend_from_slice(&cursor.head_hash);
    write_varint(&mut buf, cursor.fork_steps_seen);
    URL_SAFE_NO_PAD.encode(&buf)
}

pub fn decode(s: &str) -> Result<MainlineCursor, CursorError> {
    let raw = URL_SAFE_NO_PAD.decode(s).map_err(|_| CursorError::InvalidBase64)?;
    let min = 4 + 8 + 32 + 8 + 32 + 1;
    if raw.len() < min {
        return Err(CursorError::Truncated { expected: min, actual: raw.len() });
    }
    let mut o = 0;

    let mut chain_id_short = [0u8; 4];
    chain_id_short.copy_from_slice(&raw[o..o + 4]); o += 4;

    let mut lib_num_be = [0u8; 8];
    lib_num_be.copy_from_slice(&raw[o..o + 8]); o += 8;
    let lib_num = u64::from_be_bytes(lib_num_be);

    let mut lib_hash = [0u8; 32];
    lib_hash.copy_from_slice(&raw[o..o + 32]); o += 32;

    let mut head_num_be = [0u8; 8];
    head_num_be.copy_from_slice(&raw[o..o + 8]); o += 8;
    let head_num = u64::from_be_bytes(head_num_be);

    let mut head_hash = [0u8; 32];
    head_hash.copy_from_slice(&raw[o..o + 32]); o += 32;

    let (fork_steps_seen, consumed) = read_varint(&raw[o..])?;
    o += consumed;

    if o != raw.len() {
        return Err(CursorError::Trailing);
    }

    Ok(MainlineCursor {
        chain_id_short,
        lib_num,
        lib_hash,
        head_num,
        head_hash,
        fork_steps_seen,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fixture() -> MainlineCursor {
        MainlineCursor {
            chain_id_short: [0, 0, 0, 1],
            lib_num: 18_000_000,
            lib_hash: [0xaa; 32],
            head_num: 18_000_005,
            head_hash: [0xbb; 32],
            fork_steps_seen: 42,
        }
    }

    #[test]
    fn roundtrip() {
        let c = fixture();
        let s = encode(&c);
        let back = decode(&s).expect("decode");
        assert_eq!(c, back);
    }

    #[test]
    fn roundtrip_large_fork_steps() {
        let mut c = fixture();
        c.fork_steps_seen = u64::MAX / 2;
        let s = encode(&c);
        let back = decode(&s).expect("decode");
        assert_eq!(c, back);
    }

    #[test]
    fn rejects_invalid_base64() {
        assert_eq!(decode("not!valid!base64"), Err(CursorError::InvalidBase64));
    }

    #[test]
    fn rejects_truncated() {
        let s = URL_SAFE_NO_PAD.encode([0u8; 10]);
        match decode(&s) {
            Err(CursorError::Truncated { .. }) => (),
            other => panic!("expected Truncated, got {:?}", other),
        }
    }
}
