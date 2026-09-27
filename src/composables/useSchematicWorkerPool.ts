import type { BlockState, MeshResult } from "./schematicMeshWorker";

interface PendingTask {
  chunkPos: [number, number, number];
  blocks: Uint32Array;
  neighbors?: Uint32Array | null;
  resolve: (value: MeshResult) => void;
  reject: (reason: unknown) => void;
}

export interface SchematicWorkerPool {
  init: (resourceBase?: string) => Promise<void>;
  /** 调色板一次性下发（`blocks` 里存的是它的下标） */
  setPalette: (palette: BlockState[]) => void;
  buildMesh: (
    chunkPos: [number, number, number],
    blocks: Uint32Array,
    neighbors?: Uint32Array | null,
  ) => Promise<MeshResult>;
  dispose: () => void;
}

export function createSchematicWorkerPool(size: number): SchematicWorkerPool {
  const workers: Worker[] = [];
  const idle: Worker[] = [];
  const queue: PendingTask[] = [];
  let readyCount = 0;
  let initResolve: (() => void) | null = null;

  function handleWorkerMessage(worker: Worker, event: MessageEvent) {
    const data = event.data;
    if (data.type === "ready") {
      readyCount++;
      idle.push(worker);
      if (readyCount === workers.length && initResolve) {
        initResolve();
        initResolve = null;
      }
      return;
    }
    if (data.type === "meshResult") {
      const task = (worker as Worker & { _current?: PendingTask })._current;
      (worker as Worker & { _current?: PendingTask })._current = undefined;
      if (task) {
        task.resolve({ opaque: data.opaque, translucent: data.translucent });
      }
      const next = queue.shift();
      if (next) {
        sendToWorker(worker, next);
      } else {
        idle.push(worker);
      }
    }
  }

  function sendToWorker(worker: Worker, task: PendingTask) {
    (worker as Worker & { _current?: PendingTask })._current = task;
    // 只传 16 KiB 的方块数据（+ 邻块边界）；调色板每个 worker 只收一次
    // （否则每块重传一份，一份一千多个对象，几千个区块下来光复制就是几十万次对象拷贝）
    const transfer = [task.blocks.buffer];
    if (task.neighbors) transfer.push(task.neighbors.buffer);
    worker.postMessage(
      {
        type: "mesh",
        chunkPos: task.chunkPos,
        blocks: task.blocks,
        neighbors: task.neighbors ?? null,
      },
      transfer,
    );
  }

  function setPalette(palette: BlockState[]) {
    for (const worker of workers) {
      worker.postMessage({ type: "palette", palette });
    }
  }

  function init(resourceBase?: string): Promise<void> {
    return new Promise((resolve) => {
      initResolve = resolve;
      for (let i = 0; i < size; i++) {
        const worker = new Worker(new URL("./schematicMeshWorker.ts", import.meta.url), { type: "module" });
        worker.onmessage = (event: MessageEvent) => handleWorkerMessage(worker, event);
        worker.onerror = (e) => console.error("Schematic worker error:", e);
        workers.push(worker);
        worker.postMessage({ type: "init", resourceBase });
      }
    });
  }

  function buildMesh(
    chunkPos: [number, number, number],
    blocks: Uint32Array,
    neighbors?: Uint32Array | null,
  ): Promise<MeshResult> {
    return new Promise((resolve, reject) => {
      const task: PendingTask = { chunkPos, blocks, neighbors, resolve, reject };
      const worker = idle.pop();
      if (worker) {
        sendToWorker(worker, task);
      } else {
        queue.push(task);
      }
    });
  }

  function dispose() {
    for (const w of workers) w.terminate();
    workers.length = 0;
    idle.length = 0;
    queue.length = 0;
  }

  return { init, setPalette, buildMesh, dispose };
}
