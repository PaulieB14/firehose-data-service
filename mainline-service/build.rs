//! Compile the vendored sf.firehose.v2 protobuf into tonic stubs.
//!
//! The protos are vendored under `proto/` rather than fetched at build time
//! so the build is hermetic. Source: https://github.com/streamingfast/proto
//! (sf/firehose/v2/firehose.proto)

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let protos = [
        "proto/sf/firehose/v2/firehose.proto",
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
