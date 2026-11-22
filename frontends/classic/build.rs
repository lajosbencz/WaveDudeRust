fn main() {
	waveduderust_icon_subset::subset_icon_fonts(&["MaterialSymbolsRounded-Regular.ttf"])
		.expect("subsetting the icon font");
	println!("cargo:rerun-if-changed=assets/icons/icon.ico");
	#[cfg(windows)]
	winresource::WindowsResource::new()
		.set_icon("assets/icons/icon.ico")
		.set("ProductName", "WaveDude Rust")
		.set("FileDescription", "WaveDude Rust")
		.compile()
		.expect("embedding the Windows resources");
}
