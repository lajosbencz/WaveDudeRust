use std::fmt::Write as _;
use std::path::Path;

/// The topic folders of `examples/`, in the Examples dialog's tab order.
const TOPICS: [&str; 6] = ["sources", "lenses", "gratings", "metasurfaces", "guides", "detectors"];

/// Embeds every `examples/<topic>/*.wdproj` as `(topic, file name, text)`.
fn embed_examples() {
	let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../examples").canonicalize().expect("examples/ exists");
	println!("cargo::rerun-if-changed={}", root.display());
	let mut out = String::from("&[\n");
	for (topic, folder) in TOPICS.iter().enumerate() {
		let dir = root.join(folder);
		println!("cargo::rerun-if-changed={}", dir.display());
		let mut files: Vec<_> = std::fs::read_dir(&dir)
			.expect("a topic folder")
			.filter_map(|e| e.ok().map(|e| e.path()))
			.filter(|p| p.extension().is_some_and(|e| e == "wdproj"))
			.collect();
		files.sort();
		for path in files {
			let name = path.file_name().and_then(|n| n.to_str()).expect("a UTF-8 name");
			let _ = writeln!(out, "\t({topic}, {name:?}, include_str!({:?})),", path.display().to_string());
		}
	}
	out.push(']');
	let target = Path::new(&std::env::var("OUT_DIR").expect("OUT_DIR")).join("examples.rs");
	std::fs::write(target, out).expect("examples.rs written");
}

fn main() {
	embed_examples();
	println!("cargo::rerun-if-changed=lang");
	let config = slint_build::CompilerConfiguration::new()
		.with_bundled_translations("lang")
		.with_default_translation_context(slint_build::DefaultTranslationContext::None);
	slint_build::compile_with_config("ui/app.slint", config).expect("ui/app.slint compiles");
	println!("cargo::rerun-if-changed=assets/icons/icon.ico");
	#[cfg(windows)]
	winresource::WindowsResource::new()
		.set_icon("assets/icons/icon.ico")
		.set("ProductName", "WaveDude Rust")
		.set("FileDescription", "WaveDude Rust")
		.compile()
		.expect("embedding the Windows resources");
}
