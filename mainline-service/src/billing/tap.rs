//! TAP v2 receipt validation. Stub.
//!
//! Receipts are signed per-burst (not per-block) — §2.4 calls this out to
//! avoid signature overhead on a stream emitting thousands of blocks/minute.
//! RAV aggregation happens every 60s; on-chain collect() every hour.

#[derive(thiserror::Error, Debug)]
pub enum TapError {
    #[error("receipt verification not implemented")]
    NotImplemented,
    #[error("invalid receipt signature")]
    InvalidSignature,
    #[error("insufficient escrow")]
    InsufficientEscrow,
}

pub struct TapReceipt {
    pub allocation_id: [u8; 20],
    pub timestamp_ns: u64,
    pub nonce: u64,
    pub value: u128, // in GRT wei
    pub signature: Vec<u8>,
}

pub trait ReceiptVerifier {
    fn verify(&self, receipt: &TapReceipt) -> Result<(), TapError>;
}

pub struct StubVerifier;

impl ReceiptVerifier for StubVerifier {
    fn verify(&self, _receipt: &TapReceipt) -> Result<(), TapError> {
        Err(TapError::NotImplemented)
    }
}
