//! The scene's composition: the image or a blank canvas with the enabled parts painted in.

use crate::sync::lock;
use std::hash::{DefaultHasher, Hash, Hasher};
use std::sync::{Arc, Mutex};

use super::painter::paint_part;
use super::pixels::ScenePixels;
use crate::model::{Feed, Project, SceneMode, Stencil};

/// The decoded scene image of an image scene, None otherwise or when it does not decode.
pub fn decode_image(project: &Project) -> Option<ScenePixels> {
	let scene = &project.scene;
	if scene.mode != SceneMode::Image {
		return None;
	}
	let bytes = scene.image_data.as_deref()?;
	ScenePixels::from_image_bytes(bytes)
		.inspect_err(|e| tracing::warn!(error = %e, image = %scene.image, "scene image does not decode"))
		.ok()
}

/// The composed scene pixels: the decoded image (or None for a blank canvas) as is when no part is
/// enabled; else a copy of the image, or a blank canvas of `Scene.width x Scene.height` at the
/// background index, with the enabled parts painted in list order (later over earlier).
pub fn compose_scene(project: &Project) -> Option<ScenePixels> {
	compose_over(decode_image(project), project)
}

/// [`compose_scene`] on the project's image decoded already ([`decode_image`]).
pub fn compose_over(image: Option<ScenePixels>, project: &Project) -> Option<ScenePixels> {
	if !project.parts.iter().any(|p| p.base.enabled) {
		return image;
	}
	let scene = &project.scene;
	let mut pixels = image.unwrap_or_else(|| ScenePixels::blank(scene.width, scene.height, scene.refractive_index));
	for part in project.parts.iter().filter(|p| p.base.enabled) {
		paint_part(&mut pixels, part, &project.simulation, project.seed);
	}
	Some(pixels)
}

/// A hash of everything [`compose_scene`] reads: equal keys give equal pixels, so a caller can cache.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct SceneKey(pub u64);

impl SceneKey {
	pub fn of(project: &Project) -> Self {
		let mut h = DefaultHasher::new();
		let scene = &project.scene;
		(scene.mode == SceneMode::Image).hash(&mut h);
		if scene.mode == SceneMode::Image {
			scene.image_data.as_ref().map(image_hash).hash(&mut h);
		}
		(scene.width, scene.height, scene.refractive_index.to_bits()).hash(&mut h);
		let sim = &project.simulation;
		(sim.source_frequency_scale.to_bits(), sim.stencil == Stencil::FourthOrder).hash(&mut h);
		for part in project.parts.iter().filter(|p| p.base.enabled) {
			part.feed(&mut h);
		}
		if project.parts.iter().any(|p| p.base.enabled && p.kind.is_seeded()) {
			project.seed.hash(&mut h);
		}
		Self(h.finish())
	}
}

/// A hash of an image scene's image bytes: equal keys decode alike. None for a canvas scene.
pub fn image_key(project: &Project) -> Option<u64> {
	let scene = &project.scene;
	(scene.mode == SceneMode::Image).then(|| scene.image_data.as_ref().map(image_hash)).flatten()
}

/// The image bytes' hash, remembered for the last image: the same (shared or equal) bytes are not hashed again.
fn image_hash(bytes: &Arc<[u8]>) -> u64 {
	static LAST: Mutex<Option<(Arc<[u8]>, u64)>> = Mutex::new(None);
	let mut last = lock(&LAST);
	if let Some((cached, hash)) = &*last
		&& (Arc::ptr_eq(cached, bytes) || **cached == **bytes)
	{
		return *hash;
	}
	let mut h = DefaultHasher::new();
	bytes.hash(&mut h);
	let hash = h.finish();
	*last = Some((bytes.clone(), hash));
	hash
}

#[cfg(test)]
mod tests {
	use super::*;
	use crate::model::{Diffuser, PartKind, Rectangle};

	/// The seed is part of the scene only while a seeded part (a diffuser) is enabled.
	#[test]
	fn the_seed_keys_the_scene_of_a_diffuser() {
		let mut p = Project::default();
		p.add_part(PartKind::Rectangle(Rectangle::default()));
		let before = SceneKey::of(&p);
		p.seed += 1;
		assert_eq!(SceneKey::of(&p), before);
		let i = p.add_part(PartKind::Diffuser(Diffuser::default()));
		let with = SceneKey::of(&p);
		p.seed += 1;
		assert_ne!(SceneKey::of(&p), with);
		p.parts[i].base.enabled = false;
		let off = SceneKey::of(&p);
		p.seed += 1;
		assert_eq!(SceneKey::of(&p), off);
	}
}
