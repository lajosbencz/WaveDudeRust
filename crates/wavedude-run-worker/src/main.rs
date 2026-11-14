//! The browser's run worker (decision 0024): the live run's engine, worker, scene cache and probes in a module
//! worker with a WebGPU device of its own, posting to the page over `wavedude_app_state::remote`.
//! `run-worker.js` loads it; `tools/build-web.sh` builds it beside the app.

#[cfg(target_arch = "wasm32")]
mod worker;

#[cfg(target_arch = "wasm32")]
fn main() {
	worker::start();
}

#[cfg(not(target_arch = "wasm32"))]
fn main() {
	eprintln!("wavedude-run-worker runs in the browser, beside the page tools/build-web.sh builds");
	std::process::exit(1);
}
