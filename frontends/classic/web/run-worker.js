// The browser's run worker (decision 0024): a module worker started by the page; the wasm's start listens to it.
// The page names its build in this script's query, which its files carry too, so no cached file of another build loads.
const build = new URL(import.meta.url).search;
// wasm-feature-detect's SIMD probe (i8x16.splat, i8x16.popcnt): the SIMD build where it validates, decision 0032
const simd = WebAssembly.validate(
	new Uint8Array([
		0, 97, 115, 109, 1, 0, 0, 0, 1, 5, 1, 96, 0, 1, 123, 3, 2, 1, 0, 10, 10, 1, 8, 0, 65, 0, 253, 15, 253, 98, 11,
	]),
);
const pkg = simd ? "pkg-simd" : "pkg";
const { default: init } = await import(`./${pkg}/wavedude-run-worker.js${build}`);

await init({ module_or_path: new URL(`./${pkg}/wavedude-run-worker_bg.wasm${build}`, import.meta.url) });
