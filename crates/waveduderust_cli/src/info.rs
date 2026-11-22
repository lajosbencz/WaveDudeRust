use anyhow::Result;
use waveduderust::quality::PreviewDefaults;

pub(crate) fn run() -> Result<()> {
	let mut facts = waveduderust::run::machine_facts(&waveduderust::run::FrameStore::default_dir());
	let gpu = waveduderust::unstable::engine::gpu_adapter();
	facts.has_gpu = gpu.as_ref().map(std::result::Result::is_ok);
	println!("memory:   {:.1} GiB", facts.physical_bytes as f64 / (1u64 << 30) as f64);
	println!("threads:  {}", facts.threads);
	println!(
		"cache:    {}",
		facts.cache_drive_seek_penalty.map_or("drive kind unknown", |s| if s { "hard disk" } else { "solid state" })
	);
	match gpu {
		None => println!("gpu:      not built in (waveduderust's gpu feature is off)"),
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
