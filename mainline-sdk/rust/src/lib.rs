//! mainline-sdk — Rust consumer SDK for GRC-006 (Mainline).
//!
//! Three pieces:
//!   - `cursor`: portable `mainline-cursor-v1` encode/decode.
//!   - `tap_signer`: EIP-712 receipt signing + `x-tap-receipt` header encoding.
//!   - `client`: operator discovery + per-block attestation verification.
//!
//! Transport (tonic / grpc-web / etc.) is intentionally out of scope. The
//! `Client` works against any `OperatorTransport` impl so consumers can wire
//! their preferred gRPC stack without dragging tonic into this crate.

pub mod attestation;
pub mod client;
pub mod cursor;
pub mod tap_signer;

pub use attestation::{verify_attestation, AttestationDomain, AttestationVerifyError, MainlineAttestation, PACKED_ATTESTATION_LEN};
pub use client::{Client, ClientError, Operator, OperatorPool, OperatorTier};
pub use tap_signer::{
    digest as tap_digest, encode_header as tap_header, sign as sign_receipt, SignerError,
    TapDomain, TapReceiptV2,
};
