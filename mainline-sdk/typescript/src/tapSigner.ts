// Consumer-side TAP v2 receipt signing. Mirrors `mainline-sdk/rust/src/tap_signer.rs`
// and `mainline-service/src/billing/tap.rs` byte-for-byte.

import { secp256k1 } from "@noble/curves/secp256k1";
import { keccak_256 } from "@noble/hashes/sha3";

export interface TapDomain {
  settlementChainId: bigint;
  /** 20-byte GraphTallyCollector address. */
  verifyingContract: Uint8Array;
}

export interface TapReceiptV2 {
  /** 20-byte allocation id. */
  allocationId: Uint8Array;
  timestampNs: bigint;
  nonce: bigint;
  /** GRT wei (u128). */
  value: bigint;
  /** 65-byte (r || s || v) once signed. */
  signature: Uint8Array;
}

const enc = new TextEncoder();
const TAP_DOMAIN_TYPEHASH = enc.encode(
  "EIP712Domain(string name,string version,uint256 chainId,address verifyingContract)",
);
const TAP_RECEIPT_TYPEHASH = enc.encode(
  "Receipt(address allocation_id,uint64 timestamp_ns,uint64 nonce,uint128 value)",
);
const TAP_DOMAIN_NAME = enc.encode("TAP");
const TAP_DOMAIN_VERSION = enc.encode("2");

function keccak(b: Uint8Array): Uint8Array {
  return keccak_256(b);
}

function u256BE(value: bigint): Uint8Array {
  const out = new Uint8Array(32);
  let v = value;
  for (let i = 31; i >= 0 && v > 0n; i--) {
    out[i] = Number(v & 0xffn);
    v >>= 8n;
  }
  return out;
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

function addressWord(addr: Uint8Array): Uint8Array {
  const out = new Uint8Array(32);
  out.set(addr, 12);
  return out;
}

export function domainSeparator(d: TapDomain): Uint8Array {
  return keccak(
    concat([
      keccak(TAP_DOMAIN_TYPEHASH),
      keccak(TAP_DOMAIN_NAME),
      keccak(TAP_DOMAIN_VERSION),
      u256BE(d.settlementChainId),
      addressWord(d.verifyingContract),
    ]),
  );
}

export function structHash(r: TapReceiptV2): Uint8Array {
  return keccak(
    concat([
      keccak(TAP_RECEIPT_TYPEHASH),
      addressWord(r.allocationId),
      u256BE(r.timestampNs),
      u256BE(r.nonce),
      u256BE(r.value),
    ]),
  );
}

export function digest(d: TapDomain, r: TapReceiptV2): Uint8Array {
  return keccak(
    concat([new Uint8Array([0x19, 0x01]), domainSeparator(d), structHash(r)]),
  );
}

/**
 * Sign a receipt in place. Returns the 65-byte (r||s||v) signature with v in
 * legacy Ethereum form (27/28).
 */
export function sign(
  domain: TapDomain,
  receipt: TapReceiptV2,
  senderKey: Uint8Array,
): Uint8Array {
  if (senderKey.length !== 32) {
    throw new Error("sender key must be 32 bytes");
  }
  const d = digest(domain, receipt);
  const sig = secp256k1.sign(d, senderKey, { lowS: true, prehash: false });
  const out = new Uint8Array(65);
  const r = sig.toCompactRawBytes();
  out.set(r, 0);
  out[64] = (sig.recovery ?? 0) + 27;
  receipt.signature = out;
  return out;
}

export const RECEIPT_WIRE_VERSION = 1;
export const RECEIPT_WIRE_LEN = 1 + 20 + 8 + 8 + 16 + 65;

export function encodeWire(r: TapReceiptV2): Uint8Array {
  const out = new Uint8Array(RECEIPT_WIRE_LEN);
  let off = 0;
  out[off++] = RECEIPT_WIRE_VERSION;
  out.set(r.allocationId, off);
  off += 20;
  out.set(u64BE(r.timestampNs), off);
  off += 8;
  out.set(u64BE(r.nonce), off);
  off += 8;
  out.set(u128BE(r.value), off);
  off += 16;
  const sig = r.signature.length === 65 ? r.signature : padTo65(r.signature);
  out.set(sig, off);
  return out;
}

function u64BE(v: bigint): Uint8Array {
  const out = new Uint8Array(8);
  let x = v;
  for (let i = 7; i >= 0 && x > 0n; i--) {
    out[i] = Number(x & 0xffn);
    x >>= 8n;
  }
  return out;
}

function u128BE(v: bigint): Uint8Array {
  const out = new Uint8Array(16);
  let x = v;
  for (let i = 15; i >= 0 && x > 0n; i--) {
    out[i] = Number(x & 0xffn);
    x >>= 8n;
  }
  return out;
}

function padTo65(b: Uint8Array): Uint8Array {
  const out = new Uint8Array(65);
  out.set(b.slice(0, 65), 0);
  return out;
}

export function bytesToHex(b: Uint8Array): string {
  let s = "";
  for (const byte of b) s += byte.toString(16).padStart(2, "0");
  return s;
}

export function encodeHeader(r: TapReceiptV2): string {
  return bytesToHex(encodeWire(r));
}
