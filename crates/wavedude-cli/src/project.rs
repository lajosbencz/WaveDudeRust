use std::path::Path;

use anyhow::{Context, Result, bail};
use wavedude_core::engine::substeps::sub_steps;
use wavedude_core::io::{load_project, save_project};
use wavedude_core::model::Project;
use wavedude_core::model::normalize::Normalize;
use wavedude_core::run::{PreviewQuality, RunSpec};
use wavedude_core::scene::compose::compose_scene;
use wavedude_core::scene::glowing_edges;
use wavedude_core::scene::pixels::ScenePixels;

use crate::png_out::write_rgba;

pub fn load(path: &Path) -> Result<Project> {
	load_project(path).with_context(|| format!("loading {}", path.display()))
}

/// The composed scene, or a blank canvas when the project has neither image nor enabled parts.
pub fn scene_pixels(project: &Project) -> Result<ScenePixels> {
	if let Some(pixels) = compose_scene(project) {
		return Ok(pixels);
	}
	if project.scene.mode == wavedude_core::model::SceneMode::Image {
		bail!("the scene uses an image, but no image is loaded");
	}
	let scene = &project.scene;
	Ok(ScenePixels::blank(scene.width, scene.height, scene.refractive_index))
}

pub fn convert(input: &Path, output: &Path) -> Result<()> {
	save_project(&load(input)?, output).with_context(|| format!("saving {}", output.display()))?;
	println!("{}", output.display());
	Ok(())
}

pub fn paint(path: &Path, out: &Path, edges: bool) -> Result<()> {
	let project = load(path)?;
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

pub fn check(path: &Path) -> Result<()> {
	let project = load(path)?;
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
	let spec = RunSpec::create(&project, pixels.as_ref(), PreviewQuality::FULL, true)?;
	let scene = &spec.scene;
	println!("canvas:       {}x{}", scene.canvas_width, scene.canvas_height);
	println!("grid:         {}x{}", scene.grid_width(), scene.grid_height());
	println!("rim:          {}", scene.rim);
	println!("edges:        {:?}", scene.edges);
	println!("stride:       {}", scene.canvas_width + 2 + 2 * scene.rim);
	println!("stencil:      {:?}", spec.options.stencil);
	println!("sub-steps:    {}", sub_steps(spec.options.stencil, scene.max_c2()));
	println!("steps:        {} (a frame every {})", spec.steps, spec.visualize_every);
	println!("frames:       {}", spec.frame_count());
	println!("emitters:     {}, image sources {}", spec.emitters.len(), scene.image_sources.len());
	println!("store, full:    {}", gib(spec.store_bytes_at(PreviewQuality::FULL)));
	println!("store, default: {}", gib(spec.store_bytes_at(PreviewQuality::DEFAULT)));
	Ok(())
}
