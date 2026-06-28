// Web Worker: owns the Phorj WASM module and runs the pipeline off the main thread, so the main
// thread can terminate it on a runaway program (wasm is single-threaded and non-interruptible).
//
// Built by `wasm-pack build playground --target web --out-dir web/pkg`, which emits ./pkg/.
import init, {
  pg_check,
  pg_run,
  pg_runvm,
  pg_transpile,
  pg_explain,
  pg_lift,
} from "./pkg/phorj_playground.js";

const ready = init(); // resolves once the wasm module is instantiated

const OPS = {
  check: pg_check,
  run: pg_run,
  runvm: pg_runvm,
  transpile: pg_transpile,
  explain: pg_explain,
  lift: pg_lift, // PHP source -> Phorj draft (the inverse of transpile)
};

self.onmessage = async (e) => {
  const { id, op, arg } = e.data;
  try {
    await ready;
    const fn = OPS[op];
    if (!fn) throw new Error(`unknown op: ${op}`);
    self.postMessage({ id, ok: true, result: fn(arg) });
  } catch (err) {
    self.postMessage({ id, ok: false, error: String(err) });
  }
};
