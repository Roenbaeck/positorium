// A worker makes synchronous WASM execution interruptible without blocking UI.
// Termination discards the worker's in-memory database. Every run is seeded anew.
export class QueryRunner {
  constructor({
    timeout = 15_000,
    createWorker = () =>
      new Worker(new URL("./runner-worker.js", import.meta.url), {
        type: "module",
      }),
  } = {}) {
    this.timeout = timeout;
    this.createWorker = createWorker;
    this.sequence = 0;
    this.worker = null;
    this.pending = null;
  }

  run(script) {
    if (this.pending)
      return Promise.reject(new Error("A query is already running."));
    if (!script.trim())
      return Promise.reject(new Error("Enter a Traqula query first."));
    if (script.length > 100_000)
      return Promise.reject(
        new Error(
          "Please keep this playground query under 100,000 characters.",
        ),
      );
    return new Promise((resolve, reject) => {
      try {
        if (!this.worker) {
          const worker = this.createWorker();
          this.worker = worker;
          worker.onmessage = ({ data }) => {
            if (this.worker !== worker || data.id !== this.pending?.id) return;
            if (data.type === "result") this.settle(null, data);
            // Discard failed initialization promises as well as failed queries.
            // A retry must be able to download and initialize a fresh engine.
            else if (data.type === "error") this.stop(data.message);
          };
          worker.onerror = (event) => {
            event.preventDefault();
            if (this.worker !== worker) return;
            this.stop(
              "The browser engine could not load. Check your connection and try again.",
            );
          };
          worker.onmessageerror = () => {
            if (this.worker !== worker) return;
            this.stop(
              "The engine could not return its results. Please try again.",
            );
          };
        }
        const id = ++this.sequence;
        this.pending = {
          id,
          resolve,
          reject,
          timer: setTimeout(
            () =>
              this.stop(
                "Query stopped at the playground time limit. Try a smaller query or use Query Studio.",
              ),
            this.timeout,
          ),
        };
        this.worker.postMessage({ type: "run", id, script });
      } catch (error) {
        if (this.pending) this.stop(error.message);
        else reject(error);
      }
    });
  }

  settle(error, result) {
    const pending = this.pending;
    if (!pending) return;
    clearTimeout(pending.timer);
    this.pending = null;
    if (error) pending.reject(error);
    else pending.resolve(result);
  }

  stop(
    message = "Query stopped. Your next run starts with fresh sample data.",
  ) {
    this.worker?.terminate();
    this.worker = null;
    this.settle(new Error(message));
  }
}
