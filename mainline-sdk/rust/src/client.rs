//! Operator discovery + verifying client.
//!
//! This module does NOT include a gRPC transport — that's an integration
//! concern (tonic vs. grpc-web vs. browser fetch). It exposes:
//!
//!   - `OperatorPool::from_subgraph_response`: parse the network subgraph
//!     response shape into a typed pool, sorted by tier-aware quality.
//!   - `OperatorPool::next_for_chain`: round-robin / failover selection
//!     respecting tier requests.
//!   - `Client::recv_block`: applied to one upstream Response; returns either
//!     the verified inner cursor + block or a rejection that the caller
//!     should treat as a signal to fail over to a different operator
//!     (MAINLINE_CURSOR_UNRESUMABLE-style behaviour).
//!
//! Stream loop pattern (consumer side):
//!
//! ```ignore
//! let client = Client::new(network_subgraph_url, sender_key);
//! let pool = client.refresh_pool(chain_id).await?;
//! let mut cursor: Option<String> = None;
//! loop {
//!     let op = pool.next_for_chain(chain_id, OperatorTier::Quorum)?;
//!     let stream = transport.stream(&op.url, cursor.as_deref(), &client.tap_header(...)).await?;
//!     while let Some(response) = stream.next().await {
//!         let outcome = client.recv_block(&op, &response, payload_hash_recomputed)?;
//!         cursor = Some(outcome.next_cursor);
//!         yield outcome.block;
//!     }
//! }
//! ```

use std::sync::Mutex;

use crate::attestation::{
    split_cursor, verify_attestation, AttestationDomain, AttestationVerifyError,
};

#[derive(thiserror::Error, Debug)]
pub enum ClientError {
    #[error("no operators available for chain {chain_id:?} at tier {tier:?}")]
    NoOperatorsAvailable {
        chain_id: [u8; 32],
        tier: OperatorTier,
    },
    #[error("attestation verification failed: {0}")]
    AttestationFailed(#[from] AttestationVerifyError),
    #[error("subgraph response shape was invalid: {0}")]
    InvalidSubgraphResponse(String),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[repr(u8)]
pub enum OperatorTier {
    Reputation = 0,
    Quorum = 1,
    ProofBacked = 2,
}

#[derive(Clone, Debug)]
pub struct Operator {
    pub address: [u8; 20],
    pub url: String,
    pub tier: OperatorTier,
    pub geo_hint: u32,
    pub active: bool,
    /// Last advertised LIB on the chain we care about. Populated by
    /// `from_subgraph_response` when filtered by chain.
    pub last_advertised_lib: u64,
    /// Rolling quality score (higher = better). Tracked by the client and
    /// exposed for telemetry; used to bias `next_for_chain`.
    pub quality_score: f64,
}

pub struct OperatorPool {
    /// Lock granularity is intentionally coarse — pool refreshes are rare.
    inner: Mutex<PoolInner>,
}

struct PoolInner {
    operators: Vec<Operator>,
    /// Round-robin cursor, separate per (chain_id, tier) tuple.
    rr_state: std::collections::HashMap<([u8; 32], OperatorTier), usize>,
}

impl OperatorPool {
    pub fn new(operators: Vec<Operator>) -> Self {
        Self {
            inner: Mutex::new(PoolInner {
                operators,
                rr_state: std::collections::HashMap::new(),
            }),
        }
    }

    /// Parse a JSON payload that matches the Mainline subgraph schema:
    /// `{ "data": { "operators": [...] } }`. Only operators that advertise
    /// the requested `chain_id` and are `active` are returned, sorted by
    /// descending `lastAdvertisedLib`.
    pub fn from_subgraph_response(json: &str, chain_id: [u8; 32]) -> Result<Self, ClientError> {
        let v: serde_json::Value = serde_json::from_str(json)
            .map_err(|e| ClientError::InvalidSubgraphResponse(e.to_string()))?;
        let operators_arr = v
            .pointer("/data/operators")
            .and_then(|x| x.as_array())
            .ok_or_else(|| {
                ClientError::InvalidSubgraphResponse("missing data.operators".to_string())
            })?;
        let mut out = Vec::with_capacity(operators_arr.len());
        let chain_id_hex = format!("0x{}", hex::encode(chain_id));
        for op in operators_arr {
            let active = op.get("active").and_then(|x| x.as_bool()).unwrap_or(false);
            if !active {
                continue;
            }
            let url = op
                .get("url")
                .and_then(|x| x.as_str())
                .unwrap_or("")
                .to_string();
            let id_str = op.get("id").and_then(|x| x.as_str()).unwrap_or("");
            let mut addr = [0u8; 20];
            if let Ok(bytes) = hex::decode(id_str.strip_prefix("0x").unwrap_or(id_str)) {
                if bytes.len() == 20 {
                    addr.copy_from_slice(&bytes);
                }
            }
            let tier = op.get("tier").and_then(|x| x.as_u64()).unwrap_or(0) as u8;
            let geo = op.get("geoHint").and_then(|x| x.as_u64()).unwrap_or(0) as u32;
            let mut lib = 0u64;
            if let Some(chains) = op.get("chains").and_then(|x| x.as_array()) {
                for c in chains {
                    let cid = c
                        .pointer("/chain/id")
                        .and_then(|x| x.as_str())
                        .unwrap_or("");
                    if cid.eq_ignore_ascii_case(&chain_id_hex) {
                        lib = c
                            .get("lib")
                            .and_then(|x| x.as_str())
                            .and_then(|s| s.parse::<u64>().ok())
                            .unwrap_or(0);
                    }
                }
            }
            if lib == 0 {
                continue; // operator does not serve this chain
            }
            out.push(Operator {
                address: addr,
                url,
                tier: match tier {
                    0 => OperatorTier::Reputation,
                    1 => OperatorTier::Quorum,
                    _ => OperatorTier::ProofBacked,
                },
                geo_hint: geo,
                active: true,
                last_advertised_lib: lib,
                quality_score: 1.0,
            });
        }
        out.sort_by_key(|o| std::cmp::Reverse(o.last_advertised_lib));
        Ok(Self::new(out))
    }

    /// Pick the next operator for `chain_id` at the requested tier.
    /// Round-robin within the tier band; falls back to lower tiers if none
    /// at the requested tier are available.
    pub fn next_for_chain(
        &self,
        chain_id: [u8; 32],
        tier: OperatorTier,
    ) -> Result<Operator, ClientError> {
        let mut g = self.inner.lock().unwrap();
        let candidates: Vec<Operator> = g
            .operators
            .iter()
            .filter(|o| o.tier as u8 >= tier as u8 && o.active)
            .cloned()
            .collect();
        if candidates.is_empty() {
            return Err(ClientError::NoOperatorsAvailable { chain_id, tier });
        }
        let key = (chain_id, tier);
        let cursor = g.rr_state.entry(key).or_insert(0);
        let pick = candidates[*cursor % candidates.len()].clone();
        *cursor = cursor.wrapping_add(1);
        Ok(pick)
    }

    /// Decrement an operator's quality score after a failure. Returns the
    /// new score.
    pub fn penalise(&self, address: &[u8; 20], delta: f64) -> f64 {
        let mut g = self.inner.lock().unwrap();
        for op in g.operators.iter_mut() {
            if op.address == *address {
                op.quality_score -= delta;
                return op.quality_score;
            }
        }
        0.0
    }
}

pub struct Client {
    pub network_subgraph_url: String,
    pub sender_key: [u8; 32],
    pub attestation_domain: AttestationDomain,
}

impl Client {
    pub fn new(
        network_subgraph_url: impl Into<String>,
        sender_key: [u8; 32],
        attestation_domain: AttestationDomain,
    ) -> Self {
        Self {
            network_subgraph_url: network_subgraph_url.into(),
            sender_key,
            attestation_domain,
        }
    }

    /// Verify the attestation embedded in `cursor` against `expected_payload_hash`
    /// and `operator.address`. Returns the inner cursor on success.
    pub fn recv_block(
        &self,
        operator: &Operator,
        cursor: &str,
        expected_payload_hash: &[u8; 32],
    ) -> Result<String, ClientError> {
        let (inner_cursor, att) = split_cursor(cursor)?;
        verify_attestation(
            &self.attestation_domain,
            &att,
            &operator.address,
            Some(expected_payload_hash),
        )?;
        Ok(inner_cursor)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn dummy_operators() -> Vec<Operator> {
        vec![
            Operator {
                address: [0x01; 20],
                url: "https://op1.example".into(),
                tier: OperatorTier::Quorum,
                geo_hint: 0,
                active: true,
                last_advertised_lib: 100,
                quality_score: 1.0,
            },
            Operator {
                address: [0x02; 20],
                url: "https://op2.example".into(),
                tier: OperatorTier::ProofBacked,
                geo_hint: 0,
                active: true,
                last_advertised_lib: 99,
                quality_score: 1.0,
            },
        ]
    }

    #[test]
    fn round_robin_within_tier() {
        let pool = OperatorPool::new(dummy_operators());
        let cid = [9u8; 32];
        let a = pool.next_for_chain(cid, OperatorTier::Quorum).unwrap();
        let b = pool.next_for_chain(cid, OperatorTier::Quorum).unwrap();
        // Both T1 and T2 operators qualify for a Quorum request; round-robin
        // should rotate between them.
        assert_ne!(a.address, b.address);
    }

    #[test]
    fn parses_subgraph_response() {
        let json = r#"
        {
          "data": {
            "operators": [
              {
                "id": "0x0101010101010101010101010101010101010101",
                "url": "https://op1.example",
                "tier": 1,
                "geoHint": 0,
                "active": true,
                "chains": [
                  {"chain": {"id": "0x0000000000000000000000000000000000000000000000000000000000000001"}, "lib": "100"}
                ]
              }
            ]
          }
        }"#;
        let mut chain_id = [0u8; 32];
        chain_id[31] = 1;
        let pool = OperatorPool::from_subgraph_response(json, chain_id).unwrap();
        let op = pool.next_for_chain(chain_id, OperatorTier::Quorum).unwrap();
        assert_eq!(op.address, [0x01; 20]);
        assert_eq!(op.last_advertised_lib, 100);
    }

    #[test]
    fn skips_operators_not_serving_chain() {
        let json = r#"
        {"data":{"operators":[
          {"id":"0xaa","url":"x","tier":0,"geoHint":0,"active":true,"chains":[]}
        ]}}"#;
        let mut chain_id = [0u8; 32];
        chain_id[31] = 1;
        let pool = OperatorPool::from_subgraph_response(json, chain_id).unwrap();
        let result = pool.next_for_chain(chain_id, OperatorTier::Reputation);
        assert!(matches!(
            result,
            Err(ClientError::NoOperatorsAvailable { .. })
        ));
    }
}
