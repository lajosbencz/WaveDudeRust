//! The browser's run worker (decisions 0024, 0035): the live run's CPU engine, worker, scene cache and probes in a
//! module worker, posting to the page over `waveduderust_remote`.
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
