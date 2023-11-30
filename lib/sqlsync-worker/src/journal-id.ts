import { base58 } from "@scure/base";

export type JournalId = Uint8Array & { readonly __newtype: unique symbol };

// randomJournalId generates a random 128 bit (16 byte) JournalId
export function randomJournalId(): JournalId {
  return crypto.getRandomValues(new Uint8Array(16)) as JournalId;
}

// randomJournalId256 generates a random 256 bit (32 byte) JournalId
export function randomJournalId256(): JournalId {
  return crypto.getRandomValues(new Uint8Array(32)) as JournalId;
}

// journalIdToString converts a JournalId to a base58 encoded string
export function journalIdToString(id: JournalId): string {
  return base58.encode(id);
}

// journalIdFromString converts a base58 encoded string to a JournalId
export function journalIdFromString(s: string): JournalId {
  const bytes = base58.decode(s);
  if (bytes.length === 16 || bytes.length === 32) {
    return bytes as JournalId;
  }
  throw new Error(`invalid journal id: ${s}; must be either 16 or 32 bytes`);
}
