//! TAP v2 receipt verification and RAV aggregation. See GRC-006 §2.4.
//!
//! No new payment primitive. Reuses GraphTallyCollector / PaymentsEscrow
//! via the standard indexer-tap-agent stack.

pub mod tap;
