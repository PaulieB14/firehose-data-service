//! EIP-712 domain + signing for MainlineAttestation. Stub.

use super::MainlineAttestation;

#[derive(thiserror::Error, Debug)]
pub enum AttestationError {
    #[error("signing not implemented")]
    NotImplemented,
}

/// EIP-712 domain for Mainline attestations. The verifying contract is the
/// deployed FirehoseDataService address; the chainId is the settlement
/// chain (Arbitrum), not the data chain being attested.
pub struct AttestationDomain {
    pub settlement_chain_id: u64,
    pub verifying_contract: [u8; 20],
}

pub fn sign(
    _domain: &AttestationDomain,
    _attestation: &MainlineAttestation,
    _signing_key: &[u8; 32],
) -> Result<Vec<u8>, AttestationError> {
    // TODO: build the typed-data hash per EIP-712, sign with the operator key.
    Err(AttestationError::NotImplemented)
}
