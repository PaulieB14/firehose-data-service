// mainline-cursor-v1 encode/decode. See GRC-006 §2.7.
//
// Wire format (before base64url):
//   chainIdShort  (4 bytes, big-endian)
//   libNum        (8 bytes, big-endian u64)
//   libHash       (32 bytes)
//   headNum       (8 bytes, big-endian u64)
//   headHash      (32 bytes)
//   forkStepsSeen (varint, unsigned LEB128)
//
// Total minimum size: 85 bytes.
//
// This implementation mirrors `mainline-sdk/rust/src/cursor.rs` byte-for-byte
// so cursors are portable across SDKs.

export interface MainlineCursor {
  chainIdShort: Uint8Array; // 4 bytes
  libNum: bigint;
  libHash: Uint8Array; // 32 bytes
  headNum: bigint;
  headHash: Uint8Array; // 32 bytes
  forkStepsSeen: bigint;
}

export class CursorError extends Error {
  constructor(public readonly kind: "InvalidBase64" | "Truncated" | "Trailing" | "VarintOverflow", msg: string) {
    super(msg);
    this.name = "CursorError";
  }
}

function toBase64UrlNoPad(bytes: Uint8Array): string {
  let b64: string;
  if (typeof Buffer !== "undefined") {
    b64 = Buffer.from(bytes).toString("base64");
  } else {
    let bin = "";
    for (const byte of bytes) bin += String.fromCharCode(byte);
    b64 = btoa(bin);
  }
  return b64.replace(/\+/g, "-").replace(/\//g, "_").replace(/=+$/, "");
}

function fromBase64UrlNoPad(s: string): Uint8Array {
  const b64 = s.replace(/-/g, "+").replace(/_/g, "/");
  const pad = b64.length % 4 === 0 ? "" : "=".repeat(4 - (b64.length % 4));
  try {
    if (typeof Buffer !== "undefined") {
      return new Uint8Array(Buffer.from(b64 + pad, "base64"));
    }
    const bin = atob(b64 + pad);
    const out = new Uint8Array(bin.length);
    for (let i = 0; i < bin.length; i++) out[i] = bin.charCodeAt(i);
    return out;
  } catch {
    throw new CursorError("InvalidBase64", "invalid base64url");
  }
}

function writeUint64BE(v: bigint): Uint8Array {
  const out = new Uint8Array(8);
  let x = v;
  for (let i = 7; i >= 0; i--) {
    out[i] = Number(x & 0xffn);
    x >>= 8n;
  }
  return out;
}

function readUint64BE(bytes: Uint8Array, offset: number): bigint {
  let v = 0n;
  for (let i = 0; i < 8; i++) v = (v << 8n) | BigInt(bytes[offset + i]);
  return v;
}

function writeVarint(out: number[], v: bigint): void {
  let x = v;
  while (x >= 0x80n) {
    out.push(Number((x & 0x7fn) | 0x80n));
    x >>= 7n;
  }
  out.push(Number(x));
}

function readVarint(bytes: Uint8Array, offset: number): { value: bigint; consumed: number } {
  let result = 0n;
  let shift = 0n;
  for (let i = offset; i < bytes.length; i++) {
    if (shift >= 64n) throw new CursorError("VarintOverflow", "varint overflow");
    result |= BigInt(bytes[i] & 0x7f) << shift;
    if ((bytes[i] & 0x80) === 0) return { value: result, consumed: i - offset + 1 };
    shift += 7n;
  }
  throw new CursorError("Truncated", "truncated varint");
}

export function encode(c: MainlineCursor): string {
  if (c.chainIdShort.length !== 4) throw new Error("chainIdShort must be 4 bytes");
  if (c.libHash.length !== 32) throw new Error("libHash must be 32 bytes");
  if (c.headHash.length !== 32) throw new Error("headHash must be 32 bytes");

  const parts: number[] = [];
  for (const b of c.chainIdShort) parts.push(b);
  for (const b of writeUint64BE(c.libNum)) parts.push(b);
  for (const b of c.libHash) parts.push(b);
  for (const b of writeUint64BE(c.headNum)) parts.push(b);
  for (const b of c.headHash) parts.push(b);
  writeVarint(parts, c.forkStepsSeen);

  return toBase64UrlNoPad(new Uint8Array(parts));
}

export function decode(s: string): MainlineCursor {
  const raw = fromBase64UrlNoPad(s);
  const min = 4 + 8 + 32 + 8 + 32 + 1;
  if (raw.length < min) {
    throw new CursorError("Truncated", `expected at least ${min} bytes, got ${raw.length}`);
  }
  let o = 0;
  const chainIdShort = raw.slice(o, o + 4); o += 4;
  const libNum = readUint64BE(raw, o); o += 8;
  const libHash = raw.slice(o, o + 32); o += 32;
  const headNum = readUint64BE(raw, o); o += 8;
  const headHash = raw.slice(o, o + 32); o += 32;
  const { value: forkStepsSeen, consumed } = readVarint(raw, o);
  o += consumed;
  if (o !== raw.length) throw new CursorError("Trailing", "trailing bytes after cursor");
  return { chainIdShort, libNum, libHash, headNum, headHash, forkStepsSeen };
}
