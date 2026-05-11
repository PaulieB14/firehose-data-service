// Operator discovery + attestation-verifying client (TypeScript counterpart of
// `mainline-sdk/rust/src/client.rs`). Transport is intentionally caller-supplied.

import {
  AttestationDomain,
  splitCursor,
  verifyAttestation,
} from "./attestation";

export enum OperatorTier {
  Reputation = 0,
  Quorum = 1,
  ProofBacked = 2,
}

export interface Operator {
  address: Uint8Array; // 20 bytes
  url: string;
  tier: OperatorTier;
  geoHint: number;
  active: boolean;
  lastAdvertisedLib: bigint;
  qualityScore: number;
}

function hexAddressToBytes(s: string): Uint8Array {
  const stripped = s.startsWith("0x") ? s.slice(2) : s;
  if (stripped.length !== 40) return new Uint8Array(20);
  const out = new Uint8Array(20);
  for (let i = 0; i < 20; i++) {
    out[i] = parseInt(stripped.slice(i * 2, i * 2 + 2), 16);
  }
  return out;
}

function bytesEqualLower(a: Uint8Array, hex: string): boolean {
  const stripped = (hex.startsWith("0x") ? hex.slice(2) : hex).toLowerCase();
  if (stripped.length !== a.length * 2) return false;
  for (let i = 0; i < a.length; i++) {
    const byte = parseInt(stripped.slice(i * 2, i * 2 + 2), 16);
    if (byte !== a[i]) return false;
  }
  return true;
}

export class OperatorPool {
  private operators: Operator[];
  private rrState = new Map<string, number>();

  constructor(operators: Operator[]) {
    this.operators = operators;
  }

  /**
   * Parse the network subgraph response shape into a typed pool, filtered to
   * operators that serve `chainId` and sorted by descending last advertised LIB.
   */
  static fromSubgraphResponse(json: string, chainId: Uint8Array): OperatorPool {
    const parsed: any = JSON.parse(json);
    const list: any[] = parsed?.data?.operators ?? [];
    const out: Operator[] = [];
    for (const op of list) {
      if (!op?.active) continue;
      const chains: any[] = op.chains ?? [];
      let lib = 0n;
      for (const c of chains) {
        if (bytesEqualLower(chainId, c?.chain?.id ?? "")) {
          lib = BigInt(c.lib ?? 0);
        }
      }
      if (lib === 0n) continue;
      const tierNum = Number(op.tier ?? 0);
      out.push({
        address: hexAddressToBytes(op.id ?? ""),
        url: op.url ?? "",
        tier: tierNum as OperatorTier,
        geoHint: Number(op.geoHint ?? 0),
        active: true,
        lastAdvertisedLib: lib,
        qualityScore: 1.0,
      });
    }
    out.sort((a, b) => (b.lastAdvertisedLib > a.lastAdvertisedLib ? 1 : -1));
    return new OperatorPool(out);
  }

  nextForChain(chainId: Uint8Array, tier: OperatorTier): Operator {
    const candidates = this.operators.filter((o) => o.active && (o.tier as number) >= (tier as number));
    if (candidates.length === 0) {
      throw new Error(`no operators available for tier ${tier}`);
    }
    const key = `${Array.from(chainId).join(",")}-${tier}`;
    const cursor = this.rrState.get(key) ?? 0;
    const op = candidates[cursor % candidates.length];
    this.rrState.set(key, cursor + 1);
    return op;
  }

  penalise(address: Uint8Array, delta: number): number {
    for (const op of this.operators) {
      if (op.address.length === address.length && op.address.every((b, i) => b === address[i])) {
        op.qualityScore -= delta;
        return op.qualityScore;
      }
    }
    return 0;
  }
}

export class Client {
  constructor(
    public readonly networkSubgraphUrl: string,
    public readonly senderKey: Uint8Array,
    public readonly attestationDomain: AttestationDomain,
  ) {}

  /**
   * Strip the attestation suffix from a Stream.Blocks cursor and verify it
   * against the operator's expected signing address + the consumer-recomputed
   * payloadHash. Returns the inner cursor for resume.
   */
  recvBlock(operator: Operator, cursor: string, expectedPayloadHash: Uint8Array): string {
    const { innerCursor, attestation } = splitCursor(cursor);
    verifyAttestation(this.attestationDomain, attestation, operator.address, expectedPayloadHash);
    return innerCursor;
  }
}
