import { base58 } from "@scure/base";

export type JournalId = string;

export const JournalId = (): JournalId => {
  let bytes = crypto.getRandomValues(new Uint8Array(16));
  return base58.encode(bytes);
};

export const JournalIdToBytes = (s: JournalId): Uint8Array => {
  return base58.decode(s);
};
