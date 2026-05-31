import type { Metadata } from "@bearcove/vox-wire";
import { ClientMetadata } from "./metadata.ts";

export const RETRY_SUPPORT_METADATA_KEY = "vox-retry-support";
export const OPERATION_ID_METADATA_KEY = "vox-operation-id";
export const RETRY_SUPPORT_VERSION = 1n;

export function appendRetrySupportMetadata(metadata: Metadata): Metadata {
  if (!metadataSupportsRetry(metadata)) {
    metadata.set(RETRY_SUPPORT_METADATA_KEY, RETRY_SUPPORT_VERSION);
  }
  return metadata;
}

export function metadataSupportsRetry(metadata: Metadata): boolean {
  return metadata.get(RETRY_SUPPORT_METADATA_KEY) === RETRY_SUPPORT_VERSION;
}

export function metadataOperationId(metadata: Metadata): bigint | undefined {
  const value = metadata.get(OPERATION_ID_METADATA_KEY);
  return typeof value === "bigint" ? value : undefined;
}

export function ensureOperationId(metadata: ClientMetadata, operationId: bigint): void {
  if (metadata.has(OPERATION_ID_METADATA_KEY)) {
    return;
  }
  metadata.set(OPERATION_ID_METADATA_KEY, operationId);
}
