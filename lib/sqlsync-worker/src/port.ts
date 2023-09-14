import { WorkerToHostMsg } from "../sqlsync-wasm/pkg/sqlsync_wasm.js";

export type PortId = number;

export const nextPortId = (() => {
  let id = 0;
  return () => id++;
})();

export class Port {
  readonly port: WeakRef<MessagePort>;

  constructor(port: MessagePort) {
    this.port = new WeakRef(port);
  }

  postMessage(msg: WorkerToHostMsg) {
    this.port.deref()?.postMessage(msg);
  }

  close() {
    this.port.deref()?.close();
  }
}

export class PortRouter {
  #ports = new Map<PortId, Port>();
  #registry: FinalizationRegistry<PortId>;

  constructor() {
    this.#registry = new FinalizationRegistry((id: PortId) => {
      console.log("sqlsync: port garbage collected", id);
      this.#ports.delete(id);
    });
  }

  register(port: MessagePort): PortId {
    const id = nextPortId();
    this.#registry.register(port, id);
    this.#ports.set(id, new Port(port));
    return id;
  }

  unregister(id: PortId) {
    this.#ports.get(id)?.close();
    this.#ports.delete(id);
  }

  sendOne(portId: PortId, msg: WorkerToHostMsg) {
    console.log("sqlsync: sending message", msg, "to", portId);
    const port = this.#ports.get(portId);
    if (port) {
      port.postMessage(msg);
    } else {
      throw new SendError([portId]);
    }
  }

  sendMany(portIds: PortId[], msg: WorkerToHostMsg) {
    console.log("sqlsync: sending message", msg, "to", portIds);
    const missingPorts = [];
    for (const portId of portIds) {
      const port = this.#ports.get(portId);
      if (port) {
        this.#ports.get(portId)?.postMessage(msg);
      } else {
        missingPorts.push(portId);
      }
    }
    if (missingPorts.length > 0) {
      throw new SendError(missingPorts);
    }
  }

  sendAll(msg: WorkerToHostMsg) {
    console.log("sqlsync: broadcasting message", msg);
    for (const port of this.#ports.values()) {
      port.postMessage(msg);
    }
  }
}

export class SendError extends Error {
  constructor(readonly _missingPorts: PortId[]) {
    super(`missing ports: ${_missingPorts.join(", ")}`);
  }

  missingPorts() {
    return this._missingPorts;
  }
}
