// Per-block attestation parse + EIP-712 verify (TypeScript counterpart of
// `mainline-sdk/rust/src/attestation.rs`).

import { secp256k1 } from "@noble/curves/secp256k1";
import { keccak_256 } from "@noble/hashes/sha3";

export const CURSOR_ATTESTATION_DELIMITER = "||mainline-att||";
export const PACKED_ATTESTATION_LEN = 32 + 8 + 32 + 32 + 32 + 65;

export interface AttestationDomain {
  settlementChainId: bigint;
  /** 20-byte FirehoseDataService address. */
  verifyingContract: Uint8Array;
}

export interface MainlineAttestation {
  chainId: Uint8Array;
  blockNumber: bigint;
  blockHash: Uint8Array;
  stateRoot: Uint8Array;
  payloadHash: Uint8Array;
  signature: Uint8Array;
}

const enc = new TextEncoder();
const EIP712_DOMAIN_TYPEHASH = enc.encode(
  "EIP712Domain(string name,string version,uint256 chainId,address verifyingContract)",
);
const MAINLINE_ATTESTATION_TYPEHASH = enc.encode(
  "MainlineAttestation(bytes32 chainId,uint64 blockNumber,bytes32 blockHash,bytes32 stateRoot,bytes32 payloadHash)",
);
const DOMAIN_NAME = enc.encode("Mainline");
const DOMAIN_VERSION = enc.encode("1");

function keccak(b: Uint8Array): Uint8Array {
  return keccak_256(b);
}

function concat(parts: Uint8Array[]): Uint8Array {
  const len = parts.reduce((a, p) => a + p.length, 0);
  const out = new Uint8Array(len);
  let off = 0;
  for (const p of parts) {
    out.set(p, off);
    off += p.length;
  }
  return out;
}

function u256BE(v: bigint): Uint8Array {
  const out = new Uint8Array(32);
  let x = v;
  for (let i = 31; i >= 0 && x > 0n; i--) {
    out[i] = Number(x & 0xffn);
    x >>= 8n;
  }
  return out;
}

function addressWord(addr: Uint8Array): Uint8Array {
  const out = new Uint8Array(32);
  out.set(addr, 12);
  return out;
}

function hexToBytes(s: string): Uint8Array {
  if (s.length % 2 !== 0) throw new Error("odd-length hex");
  const out = new Uint8Array(s.length / 2);
  for (let i = 0; i < out.length; i++) {
    const byte = parseInt(s.slice(i * 2, i * 2 + 2), 16);
    if (Number.isNaN(byte)) throw new Error("invalid hex");
    out[i] = byte;
  }
  return out;
}

export function parsePacked(bytes: Uint8Array): MainlineAttestation {
  if (bytes.length < PACKED_ATTESTATION_LEN) {
    throw new Error(
      `attestation truncated (expected ${PACKED_ATTESTATION_LEN}, got ${bytes.length})`,
    );
  }
  let p = 0;
  const chainId = bytes.slice(p, p + 32);
  p += 32;
  let blockNumber = 0n;
  for (let i = 0; i < 8; i++) blockNumber = (blockNumber << 8n) | BigInt(bytes[p + i]);
  p += 8;
  const blockHash = bytes.slice(p, p + 32);
  p += 32;
  const stateRoot = bytes.slice(p, p + 32);
  p += 32;
  const payloadHash = bytes.slice(p, p + 32);
  p += 32;
  const signature = bytes.slice(p, p + 65);
  return { chainId, blockNumber, blockHash, stateRoot, payloadHash, signature };
}

export function parseHex(s: string): MainlineAttestation {
  return parsePacked(hexToBytes(s));
}

/**
 * Split a Stream.Blocks cursor on the `||mainline-att||` delimiter, returning
 * the inner cursor plus the parsed attestation.
 */
export function splitCursor(cursor: string): { innerCursor: string; attestation: MainlineAttestation } {
  const idx = cursor.lastIndexOf(CURSOR_ATTESTATION_DELIMITER);
  if (idx < 0) throw new Error("cursor missing attestation suffix");
  const inner = cursor.slice(0, idx);
  const hex = cursor.slice(idx + CURSOR_ATTESTATION_DELIMITER.length);
  return { innerCursor: inner, attestation: parseHex(hex) };
}

function domainSeparator(d: AttestationDomain): Uint8Array {
  return keccak(
    concat([
      keccak(EIP712_DOMAIN_TYPEHASH),
      keccak(DOMAIN_NAME),
      keccak(DOMAIN_VERSION),
      u256BE(d.settlementChainId),
      addressWord(d.verifyingContract),
    ]),
  );
}

function structHash(a: MainlineAttestation): Uint8Array {
  return keccak(
    concat([
      keccak(MAINLINE_ATTESTATION_TYPEHASH),
      a.chainId,
      u256BE(a.blockNumber),
      a.blockHash,
      a.stateRoot,
      a.payloadHash,
    ]),
  );
}

function digest(d: AttestationDomain, a: MainlineAttestation): Uint8Array {
  return keccak(concat([new Uint8Array([0x19, 0x01]), domainSeparator(d), structHash(a)]));
}

function recoverSigner(prehash: Uint8Array, signature: Uint8Array): Uint8Array {
  if (signature.length !== 65) throw new Error("signature must be 65 bytes");
  const v = signature[64];
  const recovery = v >= 27 ? v - 27 : v;
  const sig = secp256k1.Signature.fromCompact(signature.slice(0, 64)).addRecoveryBit(recovery);
  const pub = sig.recoverPublicKey(prehash);
  const uncompressed = pub.toRawBytes(false); // 65 bytes, leading 0x04
  const hash = keccak(uncompressed.slice(1));
  return hash.slice(12);
}

function bytesEqual(a: Uint8Array, b: Uint8Array): boolean {
  if (a.length !== b.length) return false;
  for (let i = 0; i < a.length; i++) if (a[i] !== b[i]) return false;
  return true;
}

/**
 * Verify the attestation against the expected operator signing address and an
 * optional consumer-recomputed `payloadHash`.
 */
export function verifyAttestation(
  domain: AttestationDomain,
  attestation: MainlineAttestation,
  expectedSigner: Uint8Array,
  expectedPayloadHash?: Uint8Array,
): void {
  if (expectedPayloadHash && !bytesEqual(expectedPayloadHash, attestation.payloadHash)) {
    throw new Error("payload_hash mismatch");
  }
  const pre = digest(domain, attestation);
  const recovered = recoverSigner(pre, attestation.signature);
  if (!bytesEqual(recovered, expectedSigner)) {
    throw new Error(
      `attestation signer mismatch (recovered=0x${Array.from(recovered)
        .map((b) => b.toString(16).padStart(2, "0"))
        .join("")})`,
    );
  }
}
