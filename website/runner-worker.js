import init, { WasmEngine } from "./pkg/positorium.js";

const ready = Promise.all([
  init(),
  fetch(new URL("./examples/seed.traqula", import.meta.url)).then(
    (response) => {
      if (!response.ok)
        throw new Error(
          "The sample data could not be loaded. Please reload the page.",
        );
      return response.text();
    },
  ),
]);
// Attach the rejection handler immediately; report initialization errors to the
// caller with its request id, including when the request arrives after loading.
ready.catch(() => {});

self.onmessage = async ({ data }) => {
  if (data.type !== "run") return;
  let engine;
  try {
    const [, seed] = await ready;
    engine = new WasmEngine();
    engine.execute(seed);
    const started = performance.now();
    const output = engine.execute_with_query_options(data.script, {
      query_limits: {
        max_work_units: 1_000_000,
        max_memory_bytes: 32 * 1024 * 1024,
      },
      max_rows_per_search: 100,
    });
    if (output.interface_version !== "1" || output.traqula_version !== 1) {
      throw new Error(
        "This engine uses an unsupported result format. Please reload the page.",
      );
    }
    self.postMessage({
      type: "result",
      id: data.id,
      output,
      elapsed: performance.now() - started,
    });
  } catch (error) {
    self.postMessage({
      type: "error",
      id: data.id,
      message: error instanceof Error ? error.message : String(error),
    });
  } finally {
    engine?.free();
  }
};
