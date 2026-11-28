//! Declares `cpu_wide`: set where the CPU engine's row entries (`RowKernels::run_row` in
//! engine/cpu/backend) have copies compiled for a wider instruction set, picked at run time (x86-64: AVX2). There the
//! closures between a row entry and the kernels are inlined by force, so the kernels compile into the copy and take
//! its instruction set; elsewhere the module's instruction set is the code's everywhere, and forcing them only grows
//! the binary (docs/measurements.md).
fn main() {
	println!("cargo::rerun-if-changed=build.rs");
	println!("cargo::rustc-check-cfg=cfg(cpu_wide)");
	if std::env::var("CARGO_CFG_TARGET_ARCH").as_deref() == Ok("x86_64") {
		println!("cargo::rustc-cfg=cpu_wide");
	}
}
