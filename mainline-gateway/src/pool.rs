//! Per-chain operator pool, refreshed periodically from the Mainline network
//! subgraph.

use std::sync::{Arc, Mutex};
use std::time::Duration;

use serde::{Deserialize, Serialize};

#[derive(thiserror::Error, Debug)]
pub enum PoolError {
    #[error("subgraph request failed: {0}")]
    Subgraph(String),
    #[error("subgraph response shape was invalid: {0}")]
    InvalidResponse(String),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
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
    pub last_advertised_lib: u64,
    pub quality_score: f64,
}

pub struct OperatorPool {
    pub network_subgraph_url: String,
    inner: Arc<Mutex<Vec<Operator>>>,
    pub refresh_interval: Duration,
}

impl OperatorPool {
    pub fn new(network_subgraph_url: impl Into<String>) -> Self {
        Self {
            network_subgraph_url: network_subgraph_url.into(),
            inner: Arc::new(Mutex::new(Vec::new())),
            refresh_interval: Duration::from_secs(30),
        }
    }

    pub fn with_operators(operators: Vec<Operator>) -> Self {
        Self {
            network_subgraph_url: String::new(),
            inner: Arc::new(Mutex::new(operators)),
            refresh_interval: Duration::from_secs(30),
        }
    }

    /// Replace the pool from a subgraph JSON response. Same shape as the SDK's
    /// `OperatorPool::from_subgraph_response`.
    pub fn replace_from_json(&self, json: &str, chain_id: &[u8; 32]) -> Result<usize, PoolError> {
        let v: serde_json::Value = serde_json::from_str(json)
            .map_err(|e| PoolError::InvalidResponse(e.to_string()))?;
        let arr = v
            .pointer("/data/operators")
            .and_then(|x| x.as_array())
            .ok_or_else(|| PoolError::InvalidResponse("missing data.operators".into()))?;
        let chain_id_hex = format!("0x{}", hex::encode(chain_id));
        let mut next = Vec::new();
        for op in arr {
            if !op.get("active").and_then(|x| x.as_bool()).unwrap_or(false) {
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
            let tier_num = op.get("tier").and_then(|x| x.as_u64()).unwrap_or(0);
            let tier = match tier_num {
                0 => OperatorTier::Reputation,
                1 => OperatorTier::Quorum,
                _ => OperatorTier::ProofBacked,
            };
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
                continue;
            }
            next.push(Operator {
                address: addr,
                url,
                tier,
                geo_hint: geo,
                active: true,
                last_advertised_lib: lib,
                quality_score: 1.0,
            });
        }
        let len = next.len();
        let mut g = self.inner.lock().unwrap();
        *g = next;
        Ok(len)
    }

    pub fn list(&self) -> Vec<Operator> {
        self.inner.lock().unwrap().clone()
    }

    /// Top-k operators by quality score, restricted to the requested tier and
    /// above. Used by `run_fetch_quorum` to pick the fan-out set.
    pub fn top_k(&self, k: usize, tier: OperatorTier) -> Vec<Operator> {
        let mut copy: Vec<Operator> = self
            .inner
            .lock()
            .unwrap()
            .iter()
            .filter(|o| o.active && (o.tier as u8) >= (tier as u8))
            .cloned()
            .collect();
        copy.sort_by(|a, b| b.quality_score.partial_cmp(&a.quality_score).unwrap());
        copy.truncate(k);
        copy
    }

    /// The single highest-quality operator (used for streaming).
    pub fn best_for_chain(&self, tier: OperatorTier) -> Option<Operator> {
        self.top_k(1, tier).into_iter().next()
    }

    pub fn adjust_quality(&self, address: &[u8; 20], delta: f64) {
        for op in self.inner.lock().unwrap().iter_mut() {
            if op.address == *address {
                op.quality_score += delta;
                return;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_subgraph_and_filters_inactive_or_offchain() {
        let json = r#"
        {"data":{"operators":[
          {"id":"0x0101010101010101010101010101010101010101","url":"https://op1",
           "tier":1,"geoHint":0,"active":true,
           "chains":[{"chain":{"id":"0x0000000000000000000000000000000000000000000000000000000000000001"},"lib":"100"}]},
          {"id":"0x0202020202020202020202020202020202020202","url":"https://op2",
           "tier":2,"geoHint":0,"active":false,
           "chains":[{"chain":{"id":"0x0000000000000000000000000000000000000000000000000000000000000001"},"lib":"99"}]},
          {"id":"0x0303030303030303030303030303030303030303","url":"https://op3",
           "tier":1,"geoHint":0,"active":true,
           "chains":[]}
        ]}}
        "#;
        let pool = OperatorPool::new("https://stub");
        let mut chain_id = [0u8; 32];
        chain_id[31] = 1;
        let count = pool.replace_from_json(json, &chain_id).unwrap();
        assert_eq!(count, 1);
        let ops = pool.list();
        assert_eq!(ops.len(), 1);
        assert_eq!(ops[0].address, [0x01; 20]);
    }
}
