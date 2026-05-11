// mainline-cursor-v1 encode/decode. See GRC-006 §2.7.
//
// Format:
//   base64url(
//     chainId (4 bytes) || libNum (8) || libHash (32) || headNum (8) || headHash (32) || forkSteps_seen (varint)
//   )

export interface MainlineCursor {
  chainIdShort: Uint8Array; // 4 bytes
  libNum: bigint;
  libHash: Uint8Array; // 32 bytes
  headNum: bigint;
  headHash: Uint8Array; // 32 bytes
  forkStepsSeen: bigint;
}

export function encode(_cursor: MainlineCursor): string {
  // TODO
  return "";
}

export function decode(_s: string): MainlineCursor {
  throw new Error("mainline-cursor-v1 decode not implemented");
}
