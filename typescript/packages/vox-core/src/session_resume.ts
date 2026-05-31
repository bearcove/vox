import type { Metadata } from "@bearcove/vox-wire";

export const SESSION_RESUME_KEY_METADATA_KEY = "vox-session-key";

export function appendSessionResumeKeyMetadata(metadata: Metadata, key: Uint8Array): Metadata {
  const next = new Map(metadata);
  next.set(SESSION_RESUME_KEY_METADATA_KEY, key.slice());
  return next;
}

export function metadataSessionResumeKey(
  metadata: Metadata,
): Uint8Array | null {
  const value = metadata.get(SESSION_RESUME_KEY_METADATA_KEY);
  if (value instanceof Uint8Array && value.length === 16) {
    return value.slice();
  }
  return null;
}
