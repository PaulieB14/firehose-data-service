//! Tier-2 quorum routing (§2.6).
//!
//! Strategy: fan out a `Fetch.Block` to `k` operators in parallel, group their
//! `payload_hash` responses, return the majority value, log every minority
//! signer so the operator-quality score can be adjusted downstream.

use std::collections::HashMap;

use crate::pool::Operator;

/// A bucket of operator addresses that all reported the same `payload_hash`
/// (or all errored — represented as `None`). Returned in
/// [`QuorumOutcome::NoMajority::groups`] so callers can see how the vote
/// split when no winner emerges.
pub type PayloadHashBucket = (Option<[u8; 32]>, Vec<[u8; 20]>);

#[derive(Clone, Debug)]
pub struct QuorumResult {
    pub operator: Operator,
    /// `payload_hash` claimed by this operator. `None` means the operator
    /// errored or timed out.
    pub payload_hash: Option<[u8; 32]>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum QuorumOutcome {
    /// `k_total` operators were asked; `k_responding` returned. `winners` agreed
    /// on `payload_hash`. Everything else is in `minorities`.
    Decided {
        payload_hash: [u8; 32],
        winners: Vec<[u8; 20]>,
        minorities: Vec<[u8; 20]>,
    },
    /// No majority emerged — there was a strict tie or no responders at all.
    NoMajority { groups: Vec<PayloadHashBucket> },
}

/// Given the per-operator responses from a fanned-out Fetch.Block, compute the
/// majority outcome and partition into winners / minorities. Pure function;
/// tests can drive it without any network.
pub fn run_fetch_quorum(results: Vec<QuorumResult>) -> QuorumOutcome {
    let mut buckets: HashMap<Option<[u8; 32]>, Vec<[u8; 20]>> = HashMap::new();
    for r in &results {
        buckets
            .entry(r.payload_hash)
            .or_default()
            .push(r.operator.address);
    }

    // Pick the bucket with the most entries (ignoring `None` errors when
    // there's at least one Some bucket of equal size — error counters must
    // never win over a real claim).
    let mut sorted: Vec<PayloadHashBucket> = buckets.into_iter().collect();
    sorted.sort_by(|a, b| {
        let len_cmp = b.1.len().cmp(&a.1.len());
        if len_cmp == std::cmp::Ordering::Equal {
            // Some > None when tied so we don't crown the error bucket.
            b.0.is_some().cmp(&a.0.is_some())
        } else {
            len_cmp
        }
    });

    if sorted.is_empty() {
        return QuorumOutcome::NoMajority { groups: vec![] };
    }

    let (top_hash, ref top_addrs) = sorted[0].clone();
    let top_len = top_addrs.len();

    // Strict majority required: top bucket must be strictly larger than the
    // second-place bucket.
    let runner_up_len = sorted.get(1).map(|x| x.1.len()).unwrap_or(0);
    if top_hash.is_none() || top_len == 0 || top_len <= runner_up_len {
        return QuorumOutcome::NoMajority { groups: sorted };
    }

    let payload_hash = top_hash.unwrap();
    let winners = top_addrs.clone();
    let minorities: Vec<[u8; 20]> = sorted
        .into_iter()
        .skip(1)
        .flat_map(|(_, addrs)| addrs)
        .collect();
    QuorumOutcome::Decided {
        payload_hash,
        winners,
        minorities,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::pool::{Operator, OperatorTier};

    fn op(addr: u8) -> Operator {
        Operator {
            address: [addr; 20],
            url: format!("https://{addr}"),
            tier: OperatorTier::Quorum,
            geo_hint: 0,
            active: true,
            last_advertised_lib: 100,
            quality_score: 1.0,
        }
    }

    #[test]
    fn majority_wins_with_minority_flagged() {
        let h_good = [0xaa; 32];
        let h_bad = [0xbb; 32];
        let results = vec![
            QuorumResult {
                operator: op(1),
                payload_hash: Some(h_good),
            },
            QuorumResult {
                operator: op(2),
                payload_hash: Some(h_good),
            },
            QuorumResult {
                operator: op(3),
                payload_hash: Some(h_bad),
            },
        ];
        match run_fetch_quorum(results) {
            QuorumOutcome::Decided {
                payload_hash,
                winners,
                minorities,
            } => {
                assert_eq!(payload_hash, h_good);
                assert_eq!(winners.len(), 2);
                assert_eq!(minorities, vec![[3u8; 20]]);
            }
            _ => panic!("expected Decided"),
        }
    }

    #[test]
    fn ties_produce_no_majority() {
        let h_a = [0xaa; 32];
        let h_b = [0xbb; 32];
        let results = vec![
            QuorumResult {
                operator: op(1),
                payload_hash: Some(h_a),
            },
            QuorumResult {
                operator: op(2),
                payload_hash: Some(h_b),
            },
        ];
        match run_fetch_quorum(results) {
            QuorumOutcome::NoMajority { groups } => {
                assert_eq!(groups.len(), 2);
            }
            _ => panic!("expected NoMajority"),
        }
    }

    #[test]
    fn errors_do_not_count_as_majority() {
        let results = vec![
            QuorumResult {
                operator: op(1),
                payload_hash: None,
            },
            QuorumResult {
                operator: op(2),
                payload_hash: None,
            },
            QuorumResult {
                operator: op(3),
                payload_hash: Some([0xcc; 32]),
            },
        ];
        // Two error responses outnumber the single live response, but the
        // gateway should never crown the error bucket.
        match run_fetch_quorum(results) {
            QuorumOutcome::NoMajority { .. } => {}
            QuorumOutcome::Decided { payload_hash, .. } => {
                panic!("decided on {payload_hash:?} when only one live response existed")
            }
        }
    }
}
