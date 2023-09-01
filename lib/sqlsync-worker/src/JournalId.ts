import { base58 } from "@scure/base";

declare const JournalId: unique symbol;
export type JournalId = string & { _opaque: typeof JournalId };

export const RandomJournalId = (): JournalId => {
  let bytes = crypto.getRandomValues(new Uint8Array(16));
  return JournalIdFromBytes(bytes);
};

export const JournalIdFromBytes = (bytes: Uint8Array): JournalId => {
  return base58.encode(bytes) as JournalId;
};

export const JournalIdToBytes = (s: JournalId): Uint8Array => {
  return base58.decode(s);
};
