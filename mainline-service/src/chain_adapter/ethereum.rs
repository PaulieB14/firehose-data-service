//! Ethereum L1 adapter. Wraps a firehose-ethereum endpoint.

use prost::Message;

use super::{AdapterError, BlockFingerprint, ChainAdapter};
use crate::grpc::ethereum_type;

pub struct EthereumAdapter {
    pub upstream_endpoint: String,
}

impl EthereumAdapter {
    pub const CHAIN_NAME: &'static str = "ethereum-mainnet";
    pub const EIP155_CHAIN_ID: u64 = 1;

    pub fn new(upstream_endpoint: impl Into<String>) -> Self {
        Self {
            upstream_endpoint: upstream_endpoint.into(),
        }
    }

    fn chain_id_bytes() -> [u8; 32] {
        let mut id = [0u8; 32];
        id[24..].copy_from_slice(&Self::EIP155_CHAIN_ID.to_be_bytes());
        id
    }
}

#[async_trait::async_trait]
impl ChainAdapter for EthereumAdapter {
    fn chain_id(&self) -> [u8; 32] {
        Self::chain_id_bytes()
    }
    fn firehose_proto_type(&self) -> &'static str {
        "sf.ethereum.type.v2.Block"
    }
    fn chain_name(&self) -> &'static str {
        Self::CHAIN_NAME
    }

    async fn current_lib(&self) -> Result<u64, AdapterError> {
        // TODO: open an EndpointInfo.Info call against self.upstream_endpoint.
        Err(AdapterError::NotImplemented)
    }

    /// Delegates to [`decode_evm_block_fingerprint`]. See that function for
    /// the full contract — error semantics, mis-sized hash handling, and
    /// the T1 verifier note.
    fn fingerprint(&self, payload: &[u8]) -> Result<BlockFingerprint, AdapterError> {
        decode_evm_block_fingerprint(payload)
    }
}

/// Decode an `sf.ethereum.type.v2.Block` payload and extract the three
/// fields that anchor a §2.6 attestation: block number, block hash, and
/// state-trie root. Shared across all EVM-family adapters (Ethereum L1,
/// Arbitrum, Base) since they all advertise the same firehose proto type.
///
/// Per the proto's header-view vendoring (see
/// `proto/sf/ethereum/type/v2/type.proto`) only those three fields are
/// pulled — everything else in the payload is ignored by proto3's
/// forward-compatibility rules.
///
/// Errors only when the payload is structurally not a `Block` (i.e.
/// fails prost decode). A `Block` whose `header` is absent or whose
/// `hash`/`state_root` are mis-sized just yields zeros for those
/// fields — that's adequate for T2 quorum where only `payload_hash`
/// is consensus-bound. A T1 verifier (Phase 3) MUST cross-check
/// `block_hash` + `state_root` against a canonical header proof
/// regardless of what this returns.
pub(crate) fn decode_evm_block_fingerprint(
    payload: &[u8],
) -> Result<BlockFingerprint, AdapterError> {
    let block = ethereum_type::Block::decode(payload)
        .map_err(|e| AdapterError::Decode(format!("sf.ethereum.type.v2.Block: {e}")))?;
    let block_number = block.number;
    let block_hash = bytes_to_array_32(&block.hash);
    let state_root = block
        .header
        .as_ref()
        .map(|h| bytes_to_array_32(&h.state_root))
        .unwrap_or([0u8; 32]);
    Ok(BlockFingerprint {
        block_number,
        block_hash,
        state_root,
    })
}

/// Right-pad / truncate a byte slice to exactly 32 bytes. Ethereum hashes
/// are canonically 32 bytes; anything else is structurally invalid but we
/// don't reject — see [`decode_evm_block_fingerprint`] docs.
fn bytes_to_array_32(b: &[u8]) -> [u8; 32] {
    let mut out = [0u8; 32];
    let n = b.len().min(32);
    out[..n].copy_from_slice(&b[..n]);
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::grpc::ethereum_type::{Block, BlockHeader};
    use prost::Message;

    #[test]
    fn chain_id_encodes_1_in_last_byte() {
        let a = EthereumAdapter::new("http://localhost:13042");
        let id = a.chain_id();
        assert_eq!(id[31], 1);
        assert!(id[..31].iter().all(|b| *b == 0));
    }

    #[test]
    fn fingerprint_decodes_real_block_proto() {
        let block = Block {
            hash: vec![0xaa; 32],
            number: 19_000_000,
            header: Some(BlockHeader {
                state_root: vec![0xbb; 32],
            }),
        };
        let payload = block.encode_to_vec();

        let adapter = EthereumAdapter::new("http://localhost:13042");
        let fp = adapter.fingerprint(&payload).expect("decode");
        assert_eq!(fp.block_number, 19_000_000);
        assert_eq!(fp.block_hash, [0xaa; 32]);
        assert_eq!(fp.state_root, [0xbb; 32]);
    }

    #[test]
    fn fingerprint_tolerates_missing_header_and_returns_zeros() {
        let block = Block {
            hash: vec![0xcc; 32],
            number: 100,
            header: None,
        };
        let payload = block.encode_to_vec();
        let fp = EthereumAdapter::new("x").fingerprint(&payload).unwrap();
        assert_eq!(fp.block_number, 100);
        assert_eq!(fp.block_hash, [0xcc; 32]);
        assert_eq!(fp.state_root, [0u8; 32]); // header absent → zeros
    }

    #[test]
    fn fingerprint_handles_short_hash_by_zero_padding() {
        let block = Block {
            hash: vec![0xee, 0xff], // 2 bytes only — structurally wrong but
            // we don't reject; pad right with zeros
            number: 1,
            header: Some(BlockHeader { state_root: vec![] }),
        };
        let payload = block.encode_to_vec();
        let fp = EthereumAdapter::new("x").fingerprint(&payload).unwrap();
        assert_eq!(&fp.block_hash[..2], &[0xee, 0xff][..]);
        assert!(fp.block_hash[2..].iter().all(|b| *b == 0));
    }

    #[test]
    fn fingerprint_rejects_garbage_bytes() {
        let garbage = vec![0xff; 64]; // not a valid Block protobuf
        let result = EthereumAdapter::new("x").fingerprint(&garbage);
        assert!(matches!(result, Err(AdapterError::Decode(_))));
    }
}
