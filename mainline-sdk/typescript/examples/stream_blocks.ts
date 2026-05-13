/**
 * Runnable consumer-side example mirroring `mainline-service/examples/stream_blocks.rs`,
 * but transport-agnostic.
 *
 * The SDK does NOT bundle a gRPC client — agents in the wild use `@grpc/grpc-js`
 * on Node, `grpc-web` in the browser, or a higher-level wrapper. This example
 * abstracts that with a `BlockSource` interface, so the focus stays on what the
 * SDK actually does:
 *
 *   1. Build + sign a TAP v2 receipt → encode for the `x-tap-receipt` header.
 *   2. Receive each `Stream.Blocks` response (cursor + raw payload).
 *   3. Recompute `payload_hash = sha256(payload)` locally.
 *   4. Split the cursor on `||mainline-att||`, parse the packed attestation,
 *      EIP-712 verify against the operator's expected signing address.
 *
 * Run it:
 *
 *     cd mainline-sdk/typescript
 *     npx tsx examples/stream_blocks.ts
 *
 * Drop in your real transport by swapping `demoBlockSource()` for a function
 * that yields `{ payload, cursor }` from your gRPC / grpc-web client.
 */
import { sha256 } from "@noble/hashes/sha256";
import { secp256k1 } from "@noble/curves/secp256k1";
import { keccak_256 } from "@noble/hashes/sha3";

import {
  AttestationDomain,
  CURSOR_ATTESTATION_DELIMITER,
  splitCursor,
  verifyAttestation,
} from "../src/attestation";
import {
  TapDomain,
  TapReceiptV2,
  digest as tapDigest,
  encodeHeader,
  sign as signTapReceipt,
} from "../src/tapSigner";

// ── helpers ──────────────────────────────────────────────────────────────

function bytesToHex(b: Uint8Array): string {
  let s = "";
  for (const byte of b) s += byte.toString(16).padStart(2, "0");
  return s;
}

function hexToBytes(s: string): Uint8Array {
  const stripped = s.startsWith("0x") ? s.slice(2) : s;
  const out = new Uint8Array(stripped.length / 2);
  for (let i = 0; i < out.length; i++) {
    out[i] = parseInt(stripped.slice(i * 2, i * 2 + 2), 16);
  }
  return out;
}

function addressFromPrivateKey(key: Uint8Array): Uint8Array {
  const pub = secp256k1.getPublicKey(key, false); // uncompressed, 65 bytes, leading 0x04
  return keccak_256(pub.slice(1)).slice(12);
}

// ── transport abstraction ────────────────────────────────────────────────
// Any source of `{ payload, cursor }` pairs satisfies the SDK. Swap this
// for `@grpc/grpc-js` calling `sf.firehose.v2.Stream/Blocks` in production.

interface BlockMessage {
  payload: Uint8Array;
  cursor: string;
}

interface BlockSource {
  next(): Promise<BlockMessage | null>;
}

// ── demo source ──────────────────────────────────────────────────────────
// Synthesises three valid attestations so the example runs end-to-end with no
// external server. Each "block" is just random payload bytes; the operator
// would normally have built the attestation, but we build it here so the
// signer recovery in verifyAttestation passes deterministically.

function demoBlockSource(
  operatorKey: Uint8Array,
  attestationDomain: AttestationDomain,
  chainIdBytes: Uint8Array,
  count: number,
): BlockSource {
  let i = 0;
  return {
    async next(): Promise<BlockMessage | null> {
      if (i >= count) return null;
      const blockNumber = BigInt(19_000_000 + i);
      const payload = new Uint8Array(64).fill((0x40 + i) & 0xff);
      const payloadHash = sha256(payload);

      // Synthesise an attestation that verifyAttestation will accept.
      const blockHash = sha256(new Uint8Array([0xbb, i & 0xff]));
      const stateRoot = sha256(new Uint8Array([0x5e, i & 0xff]));
      const attestationBytes = buildSignedAttestation(
        operatorKey,
        attestationDomain,
        chainIdBytes,
        blockNumber,
        blockHash,
        stateRoot,
        payloadHash,
      );

      const innerCursor = `demo-block-${i}`;
      const cursor =
        innerCursor + CURSOR_ATTESTATION_DELIMITER + bytesToHex(attestationBytes);

      i += 1;
      return { payload, cursor };
    },
  };
}

// Builds a 201-byte packed MainlineAttestation signed by `operatorKey`. Only
// used in the demo source; a real operator's mainline-service produces these.
function buildSignedAttestation(
  operatorKey: Uint8Array,
  domain: AttestationDomain,
  chainId: Uint8Array,
  blockNumber: bigint,
  blockHash: Uint8Array,
  stateRoot: Uint8Array,
  payloadHash: Uint8Array,
): Uint8Array {
  // Mirror mainline-sdk/src/attestation.ts digest() — we have to inline the
  // EIP-712 struct hashing because attestation.ts only exposes verify, not sign.
  const enc = new TextEncoder();
  const EIP712_DOMAIN_TYPEHASH = enc.encode(
    "EIP712Domain(string name,string version,uint256 chainId,address verifyingContract)",
  );
  const MAINLINE_ATTESTATION_TYPEHASH = enc.encode(
    "MainlineAttestation(bytes32 chainId,uint64 blockNumber,bytes32 blockHash,bytes32 stateRoot,bytes32 payloadHash)",
  );
  const DOMAIN_NAME = enc.encode("Mainline");
  const DOMAIN_VERSION = enc.encode("1");

  const u256BE = (v: bigint): Uint8Array => {
    const out = new Uint8Array(32);
    let x = v;
    for (let j = 31; j >= 0 && x > 0n; j--) {
      out[j] = Number(x & 0xffn);
      x >>= 8n;
    }
    return out;
  };
  const addressWord = (addr: Uint8Array): Uint8Array => {
    const out = new Uint8Array(32);
    out.set(addr, 12);
    return out;
  };
  const concat = (parts: Uint8Array[]): Uint8Array => {
    const len = parts.reduce((a, p) => a + p.length, 0);
    const out = new Uint8Array(len);
    let off = 0;
    for (const p of parts) {
      out.set(p, off);
      off += p.length;
    }
    return out;
  };

  const domainSep = keccak_256(
    concat([
      keccak_256(EIP712_DOMAIN_TYPEHASH),
      keccak_256(DOMAIN_NAME),
      keccak_256(DOMAIN_VERSION),
      u256BE(domain.settlementChainId),
      addressWord(domain.verifyingContract),
    ]),
  );
  const structHash = keccak_256(
    concat([
      keccak_256(MAINLINE_ATTESTATION_TYPEHASH),
      chainId,
      u256BE(blockNumber),
      blockHash,
      stateRoot,
      payloadHash,
    ]),
  );
  const prehash = keccak_256(
    concat([new Uint8Array([0x19, 0x01]), domainSep, structHash]),
  );

  const sig = secp256k1.sign(prehash, operatorKey, { lowS: true, prehash: false });
  const signature = new Uint8Array(65);
  signature.set(sig.toCompactRawBytes(), 0);
  signature[64] = (sig.recovery ?? 0) + 27;

  // Pack the 201-byte attestation: chain_id || block_number || block_hash ||
  // state_root || payload_hash || sig. Matches `encode_attestation()` in
  // mainline-service.
  const blockNumberBytes = new Uint8Array(8);
  let bn = blockNumber;
  for (let j = 7; j >= 0; j--) {
    blockNumberBytes[j] = Number(bn & 0xffn);
    bn >>= 8n;
  }
  return concat([
    chainId,
    blockNumberBytes,
    blockHash,
    stateRoot,
    payloadHash,
    signature,
  ]);
}

// ── main flow ────────────────────────────────────────────────────────────

async function main(): Promise<void> {
  // Devnet-friendly defaults; override with env vars in production.
  const settlementChainId = BigInt(process.env.MAINLINE_SETTLEMENT_CHAIN_ID ?? "421614");
  const fdsAddressHex =
    process.env.MAINLINE_FDS_ADDRESS ?? "0xabababababababababababababababababababab";
  const tapCollectorHex =
    process.env.MAINLINE_TAP_COLLECTOR ?? "0xcccccccccccccccccccccccccccccccccccccccc";
  const blockCount = Number(process.env.MAINLINE_BLOCK_COUNT ?? "3");

  // Demo keys — replace with real keys in production. The example derives
  // the operator's signing address from its key so the verify step passes.
  const senderKey = new Uint8Array(32).fill(0x66); // consumer signs TAP receipts
  const operatorKey = new Uint8Array(32).fill(0x55); // operator signs attestations
  const operatorAddress = addressFromPrivateKey(operatorKey);

  console.log("→ Operator address derived:", "0x" + bytesToHex(operatorAddress));

  // ── 1. Sign a TAP receipt ─────────────────────────────────────────────
  const tapDomain: TapDomain = {
    settlementChainId,
    verifyingContract: hexToBytes(tapCollectorHex),
  };
  const receipt: TapReceiptV2 = {
    allocationId: new Uint8Array(20).fill(0xaa),
    timestampNs: BigInt(Date.now()) * 1_000_000n,
    nonce: 1n,
    value: 1_000_000n, // 1 USDC in 6-decimal units, sized to cover a burst
    signature: new Uint8Array(),
  };
  signTapReceipt(tapDomain, receipt, senderKey);
  const headerHex = encodeHeader(receipt);
  console.log("→ TAP receipt signed:");
  console.log(`   allocation=0x${bytesToHex(receipt.allocationId)}`);
  console.log(`   value=${receipt.value} GRT-wei`);
  console.log(`   prehash=0x${bytesToHex(tapDigest(tapDomain, receipt))}`);
  console.log(`   → header value (x-tap-receipt): ${headerHex.slice(0, 32)}…`);

  // ── 2. Wire up the transport ──────────────────────────────────────────
  // In production, replace this with @grpc/grpc-js / grpc-web. Attach the
  // signed receipt as the `x-tap-receipt` metadata on your Stream.Blocks
  // request. Each yielded message must carry the operator's full cursor
  // (with the `||mainline-att||` suffix).
  const attestationDomain: AttestationDomain = {
    settlementChainId,
    verifyingContract: hexToBytes(fdsAddressHex),
  };
  const chainIdBytes = new Uint8Array(32); // settlement chain isn't the data chain
  // For the demo, encode chain_id = 1 (Ethereum) into the last byte. Real
  // operators use the appropriate per-chain id; see chain_adapter::ChainAdapter.
  chainIdBytes[31] = 1;

  const source = demoBlockSource(operatorKey, attestationDomain, chainIdBytes, blockCount);

  // ── 3. Stream + verify ────────────────────────────────────────────────
  console.log(`\n→ Streaming ${blockCount} blocks…\n`);
  let received = 0;
  let lastInnerCursor: string | null = null;

  for (;;) {
    const msg = await source.next();
    if (!msg) break;

    // Consumer-side payload_hash recompute — same sha256 the operator pinned.
    const recomputedHash = sha256(msg.payload);

    // Split the attestation off the cursor and verify EIP-712.
    const { innerCursor, attestation } = splitCursor(msg.cursor);
    verifyAttestation(attestationDomain, attestation, operatorAddress, recomputedHash);

    received += 1;
    lastInnerCursor = innerCursor;
    console.log(
      `✔ block #${String(received).padStart(3)}: payload=${msg.payload.length} bytes, ` +
        `sha256=${bytesToHex(recomputedHash.slice(0, 4))}…${bytesToHex(
          recomputedHash.slice(28),
        )}, cursor=${innerCursor}`,
    );
  }

  console.log(
    `\n✔ verified ${received} blocks; resume from inner cursor: ${
      lastInnerCursor ?? "(none)"
    }`,
  );
}

main().catch((err) => {
  console.error("✗ example failed:", err);
  process.exit(1);
});
