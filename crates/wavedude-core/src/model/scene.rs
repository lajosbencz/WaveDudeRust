use std::sync::Arc;

use super::{Fields, Word, ranges};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Word)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub enum SceneMode {
	#[default]
	Canvas,
	Image,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Word)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub enum Edges {
	/// Mur first-order absorbing ring.
	#[default]
	Open,
	/// Dirichlet wall.
	Reflective,
}

/// The simulated area and its border.
#[derive(Clone, Debug, PartialEq, Fields)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct Scene {
	pub mode: SceneMode,
	/// Display name of the image file.
	pub image: String,
	/// Where the image came from; relative to the project folder when inside it.
	pub image_path: String,
	/// The embedded image file bytes (PNG, JPEG, BMP); authoritative over the path. Shared, so a copy of the
	/// project is cheap. Not serialised: whoever moves a project this way moves its image beside it.
	#[field(skip)]
	#[cfg_attr(feature = "serde", serde(skip))]
	pub image_data: Option<Arc<[u8]>>,
	/// Decoded size of `image_data`, set by the loader; (0, 0) when none.
	#[field(skip)]
	pub image_size: (u32, u32),
	#[field(range = ranges::SCENE_SIZE)]
	pub width: u32,
	#[field(range = ranges::SCENE_SIZE)]
	pub height: u32,
	/// < 0: absorbing rim of that many cells outside the canvas; > 0: band inside; 0: none.
	#[field(range = ranges::SCENE_DAMPING_EDGE)]
	pub damping_edge: i32,
	pub edges: Edges,
	/// Background index of a blank canvas.
	#[field(range = ranges::INDEX)]
	pub refractive_index: f64,
	/// Green pixels emit (image scenes).
	pub image_sources: bool,
	#[field(range = ranges::SCENE_IMAGE_SOURCE_SCALE)]
	pub image_source_scale: f64,
	/// Project sources emit.
	pub object_sources: bool,
}

impl Default for Scene {
	fn default() -> Self {
		Self {
			mode: SceneMode::Canvas,
			image: String::new(),
			image_path: String::new(),
			image_data: None,
			image_size: (0, 0),
			width: 1920,
			height: 1080,
			damping_edge: -32,
			edges: Edges::Open,
			refractive_index: 1.0,
			image_sources: false,
			image_source_scale: 1.0,
			object_sources: true,
		}
	}
}

/// The settings of a scene that shape its medium beside the pixels.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SceneEdges {
	/// < 0: absorbing rim of that many cells outside the canvas; > 0: band inside; 0: none.
	pub damping_edge: i32,
	pub edges: Edges,
	/// Background index of a blank canvas.
	pub refractive_index: f64,
	/// Green pixels emit.
	pub image_sources: bool,
}

impl SceneEdges {
	/// Cells of absorbing rim per side outside the canvas.
	pub fn rim(&self) -> u32 {
		(-self.damping_edge).max(0) as u32
	}
}

impl From<&Scene> for SceneEdges {
	fn from(s: &Scene) -> Self {
		Self {
			damping_edge: s.damping_edge,
			edges: s.edges,
			refractive_index: s.refractive_index,
			image_sources: s.image_sources,
		}
	}
}

impl Scene {
	pub fn has_rim(&self) -> bool {
		self.damping_edge < 0
	}

	/// Cells of absorbing rim per side outside the canvas.
	pub fn rim(&self) -> u32 {
		SceneEdges::from(self).rim()
	}

	pub fn canvas_size(&self) -> (u32, u32) {
		if self.mode == SceneMode::Image && self.image_size != (0, 0) {
			self.image_size
		} else {
			(self.width, self.height)
		}
	}

	/// A copy without the image bytes, which are not cloned.
	pub fn without_image(&self) -> Self {
		let Self {
			mode,
			image,
			image_path,
			image_data: _,
			image_size,
			width,
			height,
			damping_edge,
			edges,
			refractive_index,
			image_sources,
			image_source_scale,
			object_sources,
		} = self;
		Self {
			mode: *mode,
			image: image.clone(),
			image_path: image_path.clone(),
			image_data: None,
			image_size: *image_size,
			width: *width,
			height: *height,
			damping_edge: *damping_edge,
			edges: *edges,
			refractive_index: *refractive_index,
			image_sources: *image_sources,
			image_source_scale: *image_source_scale,
			object_sources: *object_sources,
		}
	}

	/// Drops the image and keeps its size as the canvas size.
	pub fn remove_image(&mut self) {
		if self.image_size != (0, 0) {
			self.width = self.image_size.0;
			self.height = self.image_size.1;
		}
		self.mode = SceneMode::Canvas;
		self.image.clear();
		self.image_path.clear();
		self.image_data = None;
		self.image_size = (0, 0);
	}
}
