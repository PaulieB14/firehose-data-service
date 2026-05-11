//! Compile the vendored protos into tonic / prost stubs.
//!
//! The protos are vendored under `proto/` rather than fetched at build time
//! so the build is hermetic. Sources:
//!   - sf/firehose/v2/firehose.proto  → https://github.com/streamingfast/proto
//!   - sf/ethereum/type/v2/type.proto → see proto file header for vendoring notes
//!     (header-view subset of the upstream firehose-ethereum proto)

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let protos = [
        "proto/sf/firehose/v2/firehose.proto",
        "proto/sf/ethereum/type/v2/type.proto",
    ];

    println!("cargo:rerun-if-changed=proto");
    for p in &protos {
        println!("cargo:rerun-if-changed={}", p);
    }

    tonic_build::configure()
        .build_server(true)
        .build_client(true)
        .compile_protos(&protos, &["proto"])?;

    Ok(())
}
