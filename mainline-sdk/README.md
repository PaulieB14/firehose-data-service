# mainline-sdk

Consumer SDKs in Rust and TypeScript.

## What it does (eventually)

- Issues TAP receipts to operators (per-burst, per §2.4)
- Encodes and decodes `mainline-cursor-v1` (§2.7)
- Discovers operators via the network subgraph
- Provides a high-level `mainline::stream(chain_id, from_cursor)` API that hides operator selection

## Layout

```
mainline-sdk/
├── rust/         # Cargo crate, `mainline-sdk`
│   ├── Cargo.toml
│   └── src/
│       ├── lib.rs
│       ├── cursor.rs       # mainline-cursor-v1 encode/decode
│       └── tap_signer.rs   # consumer-side TAP receipt signing
└── typescript/   # npm package, `@graph/mainline-sdk`
    ├── package.json
    ├── tsconfig.json
    └── src/
        ├── index.ts
        ├── cursor.ts
        └── tapSigner.ts
```
