import { SqlValue } from "@orbitinghail/sqlsync-worker";
import { Row } from "./sqlsync";
import * as sha256 from "fast-sha256";

// omits the given keys from each member of the union
// https://stackoverflow.com/a/57103940/65872
// eslint-disable-next-line @typescript-eslint/no-explicit-any
export type OmitUnion<T, K extends keyof any> = T extends any ? Omit<T, K> : never;

export type NarrowTaggedEnum<E, T> = E extends { tag: T } ? E : never;

export function assertUnreachable(err: string, x: never): never {
  throw new Error(`unreachable: ${err}; got ${JSON.stringify(x)}`);
}

export function initWorker(workerUrl: string | URL): MessagePort {
  const type: WorkerType = workerUrl.toString().endsWith(".cjs") ? "classic" : "module";

  if (typeof SharedWorker !== "undefined") {
    const worker = new SharedWorker(workerUrl, { type });
    return worker.port;
  } else {
    const worker = new Worker(workerUrl, { type });
    // eslint-disable-next-line @typescript-eslint/no-explicit-any
    return worker as any as MessagePort;
  }
}

const UTF8Encoder = new TextEncoder();
export const serializeMutationAsJSON = <M>(mutation: M) => {
  const serialized = JSON.stringify(mutation);
  return UTF8Encoder.encode(serialized);
};

export function toRows<R extends Row = Row>(columns: string[], rows: SqlValue[][]): R[] {
  const out: R[] = [];
  for (const row of rows) {
    const obj: Row = {};
    for (let i = 0; i < columns.length; i++) {
      obj[columns[i]] = row[i];
    }
    out.push(obj as R);
  }
  return out;
}

export const pendingPromise = <T = undefined>(): [Promise<T>, (v: T) => void] => {
  let resolve: (v: T) => void | undefined;
  const promise = new Promise<T>((r) => {
    resolve = r;
  });
  // we know resolve is defined because the promise constructor runs syncronously
  return [promise, resolve!];
};

export const sha256Digest = async (data: Uint8Array): Promise<Uint8Array> => {
  if (crypto?.subtle?.digest) {
    const hash = await crypto.subtle.digest("SHA-256", data);
    return new Uint8Array(hash);
  }

  return Promise.resolve(sha256.hash(data));
};
