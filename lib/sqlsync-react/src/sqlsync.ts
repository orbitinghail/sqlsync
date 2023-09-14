import {
  DocEvent,
  DocId,
  DocReply,
  SqlValue,
  WorkerRequest,
  WorkerToHostMsg,
  journalIdToString,
  HandlerId,
  QueryKey,
  ConnectionStatus,
} from "@orbitinghail/sqlsync-worker";
import { NarrowTaggedEnum, OmitUnion, assertUnreachable, initWorker, toRows } from "./util";
import { ParameterizedQuery, toQueryKey } from "./sql";

export type Row = Record<string, SqlValue>;

export interface DocType<Mutation> {
  readonly reducerUrl: string | URL;
  readonly serializeMutation: (mutation: Mutation) => Uint8Array;
}

type DocReplyTag = DocReply["tag"];
type SelectDocReply<T> = NarrowTaggedEnum<DocReply, T>;

export interface QuerySubscription {
  handleRows: (rows: Row[]) => void;
  handleErr: (err: string) => void;
}

const nextHandlerId = (() => {
  let handlerId = 0;
  return () => handlerId++;
})();

export class SQLSync {
  #port: MessagePort;
  #openDocs = new Set<DocId>();
  #pendingOpens = new Map<DocId, Promise<{ tag: "Ack" }>>();
  #msgHandlers = new Map<HandlerId, (msg: DocReply) => void>();
  #querySubscriptions = new Map<QueryKey, QuerySubscription[]>();
  #connectionStatus: ConnectionStatus = "disconnected";
  #connectionStatusListeners = new Set<(status: ConnectionStatus) => void>();

  constructor(workerUrl: string | URL, wasmUrl: string | URL, coordinatorUrl?: string | URL) {
    this.#msgHandlers = new Map();
    const port = initWorker(workerUrl);
    this.#port = port;

    // We use a WeakRef here to avoid a circular reference between this.port and this.
    // This allows the SQLSync object to be garbage collected when it is no longer needed.
    const weakThis = new WeakRef(this);
    this.#port.onmessage = (msg) => {
      const thisRef = weakThis.deref();
      if (thisRef) {
        thisRef.#handleMessage(msg);
      } else {
        console.log(
          "sqlsync: dropping message; sqlsync object has been garbage collected",
          msg.data
        );
        // clean up the port
        port.postMessage({ tag: "Close", handlerId: 0 });
        port.onmessage = null;
        return;
      }
    };

    this.#boot(wasmUrl.toString(), coordinatorUrl?.toString()).catch((err) => {
      // TODO: expose this error to the app in a nicer way
      // probably through some event handlers on the SQLSync object
      console.error("sqlsync boot failed", err);
      throw err;
    });
  }

  close() {
    this.#port.onmessage = null;
    this.#port.postMessage({ tag: "Close", handlerId: 0 });
  }

  #handleMessage(event: MessageEvent) {
    const msg = event.data as WorkerToHostMsg;

    if (msg.tag === "Reply") {
      console.log("sqlsync: received reply", msg.handlerId, msg.reply);
      const handler = this.#msgHandlers.get(msg.handlerId);
      if (handler) {
        handler(msg.reply);
      } else {
        console.error("sqlsync: no handler for message", msg);
        throw new Error("no handler for message");
      }
    } else if (msg.tag === "Event") {
      this.#handleDocEvent(msg.docId, msg.evt);
    } else {
      assertUnreachable("unknown message", msg);
    }
  }

  #handleDocEvent(docId: DocId, evt: DocEvent) {
    console.log(`sqlsync: doc ${journalIdToString(docId)} received event`, evt);
    if (evt.tag === "ConnectionStatus") {
      this.#connectionStatus = evt.status;
      for (const listener of this.#connectionStatusListeners) {
        listener(evt.status);
      }
    } else if (evt.tag === "SubscriptionChanged") {
      const subscriptions = this.#querySubscriptions.get(evt.key);
      if (subscriptions) {
        for (const subscription of subscriptions) {
          subscription.handleRows(toRows(evt.columns, evt.rows));
        }
      }
    } else if (evt.tag === "SubscriptionErr") {
      const subscriptions = this.#querySubscriptions.get(evt.key);
      if (subscriptions) {
        for (const subscription of subscriptions) {
          subscription.handleErr(evt.err);
        }
      }
    } else {
      assertUnreachable("unknown event", evt);
    }
  }

  #send<T extends Exclude<DocReplyTag, "Err">>(
    expectedReplyTag: T,
    msg: OmitUnion<WorkerRequest, "handlerId">
  ): Promise<SelectDocReply<T>> {
    return new Promise((resolve, reject) => {
      const handlerId = nextHandlerId();
      const req: WorkerRequest = { ...msg, handlerId };

      console.log("sqlsync: sending message", req.handlerId, req.tag === "Doc" ? req.req : req);

      this.#msgHandlers.set(handlerId, (msg: DocReply) => {
        this.#msgHandlers.delete(handlerId);
        if (msg.tag === "Err") {
          reject(msg.err);
        } else if (msg.tag === expectedReplyTag) {
          // TODO: is it possible to get Typescript to infer this cast?
          resolve(msg as SelectDocReply<T>);
        } else {
          console.warn("sqlsync: unexpected reply", msg);
          reject(new Error(`expected ${expectedReplyTag} reply; got ${msg.tag}`));
        }
      });

      this.#port.postMessage(req);
    });
  }

  async #boot(wasmUrl: string, coordinatorUrl?: string): Promise<void> {
    await this.#send("Ack", {
      tag: "Boot",
      wasmUrl,
      coordinatorUrl,
    });
  }

  async #open<M>(docId: DocId, docType: DocType<M>): Promise<void> {
    let openPromise = this.#pendingOpens.get(docId);
    if (!openPromise) {
      openPromise = this.#send("Ack", {
        tag: "Doc",
        docId,
        req: {
          tag: "Open",
          reducerUrl: docType.reducerUrl.toString(),
        },
      });
      this.#pendingOpens.set(docId, openPromise);
    }
    await openPromise;
    this.#pendingOpens.delete(docId);
    this.#openDocs.add(docId);
  }

  async query<M, T extends Row = Row>(
    docId: DocId,
    docType: DocType<M>,
    sql: string,
    params: SqlValue[]
  ): Promise<T[]> {
    if (!this.#openDocs.has(docId)) {
      await this.#open(docId, docType);
    }

    const reply = await this.#send("RecordSet", {
      tag: "Doc",
      docId: docId,
      req: { tag: "Query", sql, params },
    });

    return toRows(reply.columns, reply.rows);
  }

  async subscribe<M>(
    docId: DocId,
    docType: DocType<M>,
    query: ParameterizedQuery,
    subscription: QuerySubscription
  ): Promise<() => void> {
    if (!this.#openDocs.has(docId)) {
      await this.#open(docId, docType);
    }
    const queryKey = await toQueryKey(query);

    // get or create subscription
    let subscriptions = this.#querySubscriptions.get(queryKey);
    if (!subscriptions) {
      subscriptions = [];
      this.#querySubscriptions.set(queryKey, subscriptions);
    }
    if (subscriptions.indexOf(subscription) === -1) {
      subscriptions.push(subscription);
    } else {
      throw new Error("sqlsync: duplicate subscription");
    }

    // send subscribe request
    await this.#send("Ack", {
      tag: "Doc",
      docId,
      req: { tag: "QuerySubscribe", key: queryKey, sql: query.sql, params: query.params },
    });

    // return unsubscribe function
    return () => {
      const subscriptions = this.#querySubscriptions.get(queryKey);
      if (!subscriptions) {
        // no subscriptions
        return;
      }
      const idx = subscriptions.indexOf(subscription);
      if (idx === -1) {
        // no subscription
        return;
      }
      subscriptions.splice(idx, 1);

      window.setTimeout(() => {
        // we want to wait a tiny bit before sending finalizing the unsubscribe
        // to handle the case that React resubscribes to the same query right away
        this.#unsubscribeIfNeeded(docId, queryKey).catch((err) => {
          console.error("sqlsync: error unsubscribing", err);
        });
      }, 10);
    };
  }

  async #unsubscribeIfNeeded(docId: DocId, queryKey: QueryKey): Promise<void> {
    const subscriptions = this.#querySubscriptions.get(queryKey);
    if (subscriptions instanceof Array && subscriptions.length === 0) {
      // query subscription is still registered but has no subscriptions on our side
      // inform the worker that we are no longer interested in this query
      this.#querySubscriptions.delete(queryKey);

      if (this.#openDocs.has(docId)) {
        await this.#send("Ack", {
          tag: "Doc",
          docId,
          req: { tag: "QueryUnsubscribe", key: queryKey },
        });
      }
    }
  }

  async mutate<M>(docId: DocId, docType: DocType<M>, mutation: M): Promise<void> {
    if (!this.#openDocs.has(docId)) {
      await this.#open(docId, docType);
    }
    await this.#send("Ack", {
      tag: "Doc",
      docId,
      req: { tag: "Mutate", mutation: docType.serializeMutation(mutation) },
    });
  }

  get connectionStatus(): ConnectionStatus {
    return this.#connectionStatus;
  }

  addConnectionStatusListener(listener: (status: ConnectionStatus) => void): () => void {
    this.#connectionStatusListeners.add(listener);
    return () => {
      this.#connectionStatusListeners.delete(listener);
    };
  }

  async setConnectionEnabled<M>(
    docId: DocId,
    docType: DocType<M>,
    enabled: boolean
  ): Promise<void> {
    if (!this.#openDocs.has(docId)) {
      await this.#open(docId, docType);
    }
    await this.#send("Ack", {
      tag: "Doc",
      docId,
      req: { tag: "SetConnectionEnabled", enabled },
    });
  }
}
