//! mainline-cursor-v1 encode/decode. See GRC-006 §2.7.
//!
//! Format:
//!   base64url(
//!     chainId (4 bytes) || libNum (8) || libHash (32) || headNum (8) || headHash (32) || forkSteps_seen (varint)
//!   )

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

#[derive(Error, Debug)]
pub enum CursorError {
    #[error("cursor decode not implemented")]
    NotImplemented,
    #[error("invalid base64")]
    InvalidBase64,
    #[error("truncated")]
    Truncated,
}

pub fn encode(_cursor: &MainlineCursor) -> String {
    // TODO
    String::new()
}

pub fn decode(_s: &str) -> Result<MainlineCursor, CursorError> {
    Err(CursorError::NotImplemented)
}
