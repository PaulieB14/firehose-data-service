//! gRPC surface — re-exports the upstream sf.firehose.v2 protobuf contract
//! unchanged. Per GRC-006 §2.2 this is non-negotiable.
//!
//! Generated code lives in `pb::sf::firehose::v2` and `pb::sf::ethereum::type::v2`
//! after build.rs runs.

pub mod server;

pub mod pb {
    pub mod sf {
        pub mod firehose {
            pub mod v2 {
                tonic::include_proto!("sf.firehose.v2");
            }
        }
        pub mod ethereum {
            pub mod types {
                pub mod v2 {
                    tonic::include_proto!("sf.ethereum.types.v2");
                }
            }
        }
    }
}

pub use pb::sf::ethereum::types::v2 as ethereum_type;
pub use pb::sf::firehose::v2 as firehose;
