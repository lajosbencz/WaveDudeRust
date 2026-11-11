fn main() {
	println!("cargo:rerun-if-changed=assets/icons/icon.ico");
	#[cfg(windows)]
	winresource::WindowsResource::new()
		.set_icon("assets/icons/icon.ico")
		.set("ProductName", "WaveDude Rust")
		.set("FileDescription", "WaveDude Rust")
		.compile()
		.expect("embedding the Windows resources");
}
