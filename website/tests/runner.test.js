import { test } from "node:test";
import assert from "node:assert/strict";
import { QueryRunner } from "../runner-client.js";

function fixture(options = {}) {
  const workers = [];
  const runner = new QueryRunner({
    ...options,
    createWorker() {
      const worker = {
        postMessage(message) {
          this.request = message;
        },
        terminate() {
          this.terminated = true;
        },
        reply(data) {
          this.onmessage({ data: { id: this.request.id, ...data } });
        },
      };
      workers.push(worker);
      return worker;
    },
  });
  return { runner, workers };
}

test("stopping a run rejects it, terminates execution, and the next run uses a new worker", async () => {
  const { runner, workers } = fixture();
  const first = runner.run("search [*, *, *];");
  const stopped = assert.rejects(first, /Query stopped/);
  runner.stop();
  await stopped;
  assert.equal(workers[0].terminated, true);
  const second = runner.run("search [*, *, *];");
  // A late message from the old worker must not resolve the new request.
  workers[0].reply({ type: "result", output: "stale" });
  workers[0].onerror({ preventDefault() {} });
  assert.notEqual(runner.pending, null);
  workers[1].reply({ type: "result", output: "fresh" });
  assert.equal((await second).output, "fresh");
});

test("the deadline terminates an unresponsive worker", async () => {
  const { runner, workers } = fixture({ timeout: 10 });
  await assert.rejects(runner.run("search [*, *, *];"), /time limit/);
  assert.equal(workers[0].terminated, true);
  assert.equal(runner.pending, null);
});

test("worker load failure is recoverable on a later run", async () => {
  const { runner, workers } = fixture();
  const failed = runner.run("search [*, *, *];");
  const rejected = assert.rejects(failed, /could not load/);
  workers[0].onerror({ preventDefault() {} });
  await rejected;
  assert.equal(workers[0].terminated, true);
  const retry = runner.run("search [*, *, *];");
  workers[1].reply({ type: "result", output: "recovered" });
  assert.equal((await retry).output, "recovered");
});

test("invalid input and concurrent submissions cannot replace the active request", async () => {
  const { runner, workers } = fixture();
  await assert.rejects(runner.run("  "), /Enter a Traqula/);
  await assert.rejects(runner.run("x".repeat(100_001)), /100,000/);
  assert.equal(workers.length, 0);
  const active = runner.run("search [*, *, *];");
  await assert.rejects(runner.run("another query"), /already running/);
  workers[0].reply({ type: "result", output: "original" });
  assert.equal((await active).output, "original");
});
