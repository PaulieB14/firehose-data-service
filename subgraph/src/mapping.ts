// Mainline subgraph mappings. Stub.
//
// Wire each handler against the schema in ../schema.graphql.

import {
  ChainRegistered,
  IndexerRegistered,
  ChainAdvertised,
  ServiceStarted,
  ServiceStopped,
  PaymentCollected,
  IndexerSlashed,
} from "../generated/FirehoseDataService/FirehoseDataService";

export function handleChainRegistered(_event: ChainRegistered): void {
  // TODO
}

export function handleIndexerRegistered(_event: IndexerRegistered): void {
  // TODO
}

export function handleChainAdvertised(_event: ChainAdvertised): void {
  // TODO
}

export function handleServiceStarted(_event: ServiceStarted): void {
  // TODO
}

export function handleServiceStopped(_event: ServiceStopped): void {
  // TODO
}

export function handlePaymentCollected(_event: PaymentCollected): void {
  // TODO
}

export function handleIndexerSlashed(_event: IndexerSlashed): void {
  // TODO
}
