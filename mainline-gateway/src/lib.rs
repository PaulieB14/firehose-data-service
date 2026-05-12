//! mainline-gateway crate root. The gateway runs the §2.6 Tier-2 (quorum)
//! verification on behalf of consumers that don't want to do per-block
//! attestation work themselves. The crate ships as both a binary and a
//! library so the quorum + scoring core can be reused in tests.

pub mod gateway;
pub mod pool;
pub mod quality;
pub mod quorum;

pub use gateway::GatewayService;
pub use pool::{Operator, OperatorPool, OperatorTier};
pub use quality::{QualityMetrics, QualityWindow};
pub use quorum::{run_fetch_quorum, QuorumOutcome, QuorumResult};
