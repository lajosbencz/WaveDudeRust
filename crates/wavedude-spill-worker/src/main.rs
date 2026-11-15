//! The browser's spill worker (decision 0020): the frames past a store's memory in the origin private file system,
//! written and read through synchronous access handles, which only a dedicated worker has. `spill-worker.js` loads
//! it; `tools/build-web.sh` builds it beside the app.

#[cfg(target_arch = "wasm32")]
mod worker;

#[cfg(target_arch = "wasm32")]
fn main() {
	worker::start();
}

#[cfg(not(target_arch = "wasm32"))]
fn main() {
	eprintln!("wavedude-spill-worker runs in the browser, beside the page tools/build-web.sh builds");
	std::process::exit(1);
}
