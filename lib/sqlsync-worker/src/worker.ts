import init, { DocReply, HandlerId, WorkerApi } from "../sqlsync-wasm/pkg/sqlsync_wasm.js";
import { WorkerRequest } from "./index";
import { PortId, PortRouter } from "./port";
import { assertUnreachable } from "./util";

const ports = new PortRouter();
let workerApi: WorkerApi | null = null;

function reply(portId: PortId, handlerId: HandlerId, reply: DocReply) {
  ports.sendOne(portId, { tag: "Reply", handlerId, reply });
}

interface Message {
  portId: PortId;
  req: WorkerRequest;
}

const MessageQueue = (() => {
  let queue = Promise.resolve();
  return {
    push: (m: Message) => {
      queue = queue.then(
        () => handleMessage(m),
        (e) => {
          const err = e instanceof Error ? e.message : `error: ${JSON.stringify(e)}`;
          reply(m.portId, m.req.handlerId, { tag: "Err", err });
        },
      );
    },
  };
})();

// if we are running in a dedicated worker, shim a Port
// we have to make this check inverted because
// WorkerGlobalScope extends SharedWorkerGlobalScope
if (typeof SharedWorkerGlobalScope === "undefined" || !(self instanceof SharedWorkerGlobalScope)) {
  // biome-ignore lint/suspicious/noExplicitAny: self extends messageport if we are not a shared worker
  const port = self as any as MessagePort;
  const portId = ports.register(port);
  port.addEventListener("message", (e) =>
    MessageQueue.push({ portId, req: e.data as WorkerRequest }),
  );
  console.log("sqlsync: handled dedicated worker connection; portId", portId);
}

addEventListener("connect", (e: Event) => {
  const evt = e as MessageEvent;
  const port = evt.ports[0];
  const portId = ports.register(port);
  port.addEventListener("message", (e) =>
    MessageQueue.push({ portId, req: e.data as WorkerRequest }),
  );
  port.start();
  console.log("sqlsync: received connection from tab; portId", portId);
});

// MessageQueue ensures that this async function is never run concurrently
async function handleMessage({ portId, req }: Message) {
  console.log("sqlsync: received message", req);

  if (req.tag === "Boot") {
    if (!workerApi) {
      console.log("sqlsync: initializing wasm");
      await init(req.wasmUrl);
      workerApi = new WorkerApi(ports, req.coordinatorUrl);
      console.log("sqlsync: wasm initialized");
    } else {
      // TODO(UPGRADE): if a new boot request comes in with different params we
      // should trigger a worker upgrade
      console.warn("sqlsync: ignoring duplicate boot request");
    }
    reply(portId, req.handlerId, { tag: "Ack" });
  } else if (req.tag === "Doc") {
    if (!workerApi) {
      throw new Error("not booted");
    }
    await workerApi.handle({ portId, ...req });
  } else if (req.tag === "Close") {
    console.log("sqlsync: Received close request from port", portId);
    ports.unregister(portId);
  } else {
    assertUnreachable("unknown message", req);
  }
}
