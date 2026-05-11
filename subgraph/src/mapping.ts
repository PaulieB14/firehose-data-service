// Mainline network subgraph mappings (GRC-006).
//
// Indexes events from FirehoseDataService.sol so the gateway, SDK, and any
// TAP verifier off-chain can discover live operators and their per-chain
// last-irreversible-blocks.

import { Address, BigInt, Bytes } from "@graphprotocol/graph-ts";

import {
  ChainRegistered,
  MainlineIndexerRegistered,
  ChainAdvertised,
  MainlineServiceStarted,
  MainlineServiceStopped,
  ServicePaymentCollected,
  ServiceProviderSlashed,
  PaymentsDestinationSet,
} from "../generated/FirehoseDataService/FirehoseDataService";

import {
  Operator,
  Chain,
  AdvertisedLib,
  PaymentEvent,
  SlashEvent,
  DestinationChange,
} from "../generated/schema";

const ZERO_ADDRESS = Bytes.fromHexString("0x0000000000000000000000000000000000000000") as Bytes;

function loadOrInitOperator(addr: Address, timestamp: BigInt, blockNumber: BigInt): Operator {
  let id = addr as Bytes;
  let op = Operator.load(id);
  if (op == null) {
    op = new Operator(id);
    op.url = "";
    op.tier = 0;
    op.geoHint = 0;
    op.paymentsDestination = ZERO_ADDRESS;
    op.active = false;
    op.registeredAt = timestamp;
    op.registeredAtBlock = blockNumber;
  }
  return op as Operator;
}

function advertisedLibId(operator: Address, chainId: Bytes): string {
  return operator.toHexString() + "-" + chainId.toHexString();
}

export function handleChainRegistered(event: ChainRegistered): void {
  let chain = new Chain(event.params.chainId);
  let manifest = event.params.manifest;
  chain.genesisBlock = manifest.genesisBlock;
  chain.genesisHash = manifest.genesisHash;
  chain.firehoseProtoType = manifest.firehoseProtoType;
  chain.firstStreamableBlock = manifest.firstStreamableBlock;
  chain.reorgDepth = manifest.reorgDepth.toI32();
  chain.supportsFetch = manifest.supportsFetch;
  chain.registeredAt = event.block.timestamp;
  chain.registeredAtBlock = event.block.number;
  chain.save();
}

export function handleIndexerRegistered(event: MainlineIndexerRegistered): void {
  let op = loadOrInitOperator(event.params.indexer, event.block.timestamp, event.block.number);
  op.url = event.params.url;
  op.tier = event.params.tier;
  op.geoHint = event.params.geoHint.toI32();
  op.registeredAt = event.block.timestamp;
  op.registeredAtBlock = event.block.number;
  op.save();
  // PaymentsDestinationSet fires in the same tx during register(); its handler
  // overwrites op.paymentsDestination there.
}

export function handleChainAdvertised(event: ChainAdvertised): void {
  let op = loadOrInitOperator(event.params.indexer, event.block.timestamp, event.block.number);
  op.save();

  let id = advertisedLibId(event.params.indexer, event.params.chainId);
  let entry = AdvertisedLib.load(id);
  if (entry == null) {
    entry = new AdvertisedLib(id);
    entry.operator = op.id;
    entry.chain = event.params.chainId;
  }
  entry.lib = event.params.lib;
  entry.updatedAt = event.block.timestamp;
  entry.updatedAtBlock = event.block.number;
  entry.save();
}

export function handleServiceStarted(event: MainlineServiceStarted): void {
  let op = loadOrInitOperator(event.params.indexer, event.block.timestamp, event.block.number);
  op.active = true;
  op.save();
}

export function handleServiceStopped(event: MainlineServiceStopped): void {
  let op = loadOrInitOperator(event.params.indexer, event.block.timestamp, event.block.number);
  op.active = false;
  op.save();
}

export function handlePaymentCollected(event: ServicePaymentCollected): void {
  let op = loadOrInitOperator(event.params.serviceProvider, event.block.timestamp, event.block.number);
  op.save();

  let id = event.transaction.hash.concatI32(event.logIndex.toI32());
  let payment = new PaymentEvent(id);
  payment.operator = op.id;
  payment.paymentType = event.params.feeType;
  payment.tokens = event.params.tokens;
  payment.timestamp = event.block.timestamp;
  payment.blockNumber = event.block.number;
  payment.txHash = event.transaction.hash;
  payment.save();
}

export function handleIndexerSlashed(event: ServiceProviderSlashed): void {
  let op = loadOrInitOperator(event.params.serviceProvider, event.block.timestamp, event.block.number);
  op.save();

  let id = event.transaction.hash.concatI32(event.logIndex.toI32());
  let slash = new SlashEvent(id);
  slash.operator = op.id;
  slash.tokens = event.params.tokens;
  slash.timestamp = event.block.timestamp;
  slash.blockNumber = event.block.number;
  slash.txHash = event.transaction.hash;
  slash.save();
}

export function handlePaymentsDestinationSet(event: PaymentsDestinationSet): void {
  let op = loadOrInitOperator(event.params.indexer, event.block.timestamp, event.block.number);
  op.paymentsDestination = event.params.destination as Bytes;
  op.save();

  let id = event.transaction.hash.concatI32(event.logIndex.toI32());
  let change = new DestinationChange(id);
  change.operator = op.id;
  change.destination = event.params.destination as Bytes;
  change.timestamp = event.block.timestamp;
  change.blockNumber = event.block.number;
  change.txHash = event.transaction.hash;
  change.save();
}
