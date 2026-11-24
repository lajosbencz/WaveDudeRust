use std::path::Path;

use anyhow::{Context, Result, bail};
use waveduderust::io::schemes::SchemeStore;
use waveduderust::io::{load_project, save_project};
use waveduderust::model::normalize::Normalize;
use waveduderust::model::{Project, View};
use waveduderust::quality::PreviewQuality;
use waveduderust::unstable::engine::substeps::sub_steps;
use waveduderust::unstable::scene::compose::compose_scene;
use waveduderust::unstable::scene::glowing_edges;
use waveduderust::unstable::scene::pixels::ScenePixels;
use waveduderust::unstable::spec::RunSpec;
use waveduderust_platform::settings::AppDirs;
use waveduderust_platform::user_schemes;

use crate::png_out::write_rgba;

pub(crate) fn load(path: &Path) -> Result<(Project, View)> {
	load_project(path).with_context(|| format!("loading {}", path.display()))
}

/// The user's colour schemes in the app's folders `dirs`; none without them (a platform without a home).
pub(crate) fn user_schemes(dirs: Option<&AppDirs>) -> SchemeStore {
	match dirs {
		Some(dirs) => user_schemes::load(&dirs.store()),
		None => {
			eprintln!("no user folders on this platform: the user's colour schemes are not loaded");
			SchemeStore::default()
		}
	}
}

/// The composed scene, or a blank canvas when the project has neither image nor enabled parts.
pub(crate) fn scene_pixels(project: &Project) -> Result<ScenePixels> {
	if let Some(pixels) = compose_scene(project) {
		return Ok(pixels);
	}
	if project.scene.mode == waveduderust::model::SceneMode::Image {
		bail!("the scene uses an image, but no image is loaded");
	}
	let scene = &project.scene;
	Ok(ScenePixels::blank(scene.width, scene.height, scene.refractive_index))
}

pub(crate) fn convert(input: &Path, output: &Path) -> Result<()> {
	let (project, view) = load(input)?;
	save_project(&project, &view, output).with_context(|| format!("saving {}", output.display()))?;
	println!("{}", output.display());
	Ok(())
}

pub(crate) fn paint(path: &Path, out: &Path, edges: bool) -> Result<()> {
	let (project, _) = load(path)?;
	let pixels = scene_pixels(&project)?;
	let (width, height) = (pixels.width, pixels.height);
	if edges {
		let picture = glowing_edges::compute(&pixels.rgba, width as usize, height as usize);
		write_rgba(out, width, height, &picture)?;
	} else {
		write_rgba(out, width, height, &pixels.rgba)?;
	}
	println!("{} ({width}x{height})", out.display());
	Ok(())
}

/// Lines of the debug text that differ after another normalisation pass; the reader normalises already.
fn second_pass_changes(project: &Project) -> usize {
	let text = |p: &Project| {
		let mut p = p.clone();
		p.scene.image_data = None;
		format!("{p:#?}")
	};
	let mut again = project.clone();
	again.normalize();
	let (before, after) = (text(project), text(&again));
	before.lines().zip(after.lines()).filter(|(a, b)| a != b).count()
		+ before.lines().count().abs_diff(after.lines().count())
}

fn gib(bytes: u64) -> String {
	format!("{bytes} bytes ({:.2} GiB)", bytes as f64 / (1u64 << 30) as f64)
}

pub(crate) fn check(path: &Path) -> Result<()> {
	let (project, view) = load(path)?;
	println!("project:      {}", path.display());
	println!("normalised:   loaded normalised, a second pass changed {} lines", second_pass_changes(&project));
	println!(
		"contents:     {} sources, {} parts, {} probes, {} colour schemes",
		project.sources.len(),
		project.parts.len(),
		project.probes.len(),
		project.color_schemes.len()
	);
	let pixels = compose_scene(&project);
	let spec = RunSpec::create(&project, pixels.as_ref(), PreviewQuality::FULL, (&view).into(), true)?;
	let scene = &spec.scene;
	println!("canvas:       {}x{}", scene.canvas_width, scene.canvas_height);
	println!("grid:         {}x{}", scene.grid_width(), scene.grid_height());
	println!("rim:          {}", scene.rim);
	println!("edges:        {:?}", scene.edges);
	println!("stride:       {}", scene.canvas_width + 2 + 2 * scene.rim);
	println!("stencil:      {:?}", spec.options.stencil);
	println!("sub-steps:    {}", sub_steps(spec.options.stencil, scene.max_c2()));
	println!("steps:        {} (a frame every {})", spec.schedule.steps, spec.schedule.visualize_every);
	println!("frames:       {}", spec.schedule.frame_count());
	println!("emitters:     {}, image sources {}", spec.emitters.len(), scene.image_sources.len());
	let request = waveduderust::unstable::spec::RunRequest::new(&project, PreviewQuality::FULL, (&view).into(), true);
	println!("store, full:    {}", gib(request.store_bytes_at(PreviewQuality::FULL)));
	println!("store, default: {}", gib(request.store_bytes_at(PreviewQuality::DEFAULT)));
	Ok(())
}
