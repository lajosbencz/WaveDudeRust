use anyhow::Result;
use wavedude_core::run::{MachineFacts, PreviewDefaults};

pub(crate) fn run() -> Result<()> {
	let mut facts = MachineFacts::read();
	let gpu = wavedude_core::engine::gpu_adapter();
	facts.has_gpu = gpu.as_ref().map(std::result::Result::is_ok);
	println!("memory:   {:.1} GiB", facts.physical_bytes as f64 / (1u64 << 30) as f64);
	println!("threads:  {}", facts.threads);
	match gpu {
		None => println!("gpu:      not built in (wavedude-core's gpu feature is off)"),
		Some(Ok(name)) => println!("gpu:      {name}"),
		Some(Err(e)) => println!("gpu:      none ({e})"),
	}
	let tier = PreviewDefaults::tier(&facts);
	let (quality, dither_bits) = PreviewDefaults::tier_of(&facts);
	println!("tier:     {tier:?}");
	println!(
		"quality:  resolution 1/{}, frames 1/{}, samples {:?}, dithering {}",
		quality.resolution_divisor,
		quality.frame_divisor,
		quality.format,
		if dither_bits == 0 { "off".to_owned() } else { format!("{dither_bits} bits") },
	);
	Ok(())
}
