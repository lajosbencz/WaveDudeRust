// The browser's run worker (decision 0024): a module worker started by the page; the wasm's start listens to it.
import init from "./pkg/wavedude-run-worker.js";

await init();
