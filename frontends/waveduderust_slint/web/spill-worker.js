// The browser's spill worker (decision 0020): a module worker started by the page; the wasm's start listens to it.
// The page names its build in this script's query, which its files carry too, so no cached file of another build loads.
const build = new URL(import.meta.url).search;
const { default: init } = await import(`./pkg/wavedude-spill-worker.js${build}`);

await init({ module_or_path: new URL(`./pkg/wavedude-spill-worker_bg.wasm${build}`, import.meta.url) });
