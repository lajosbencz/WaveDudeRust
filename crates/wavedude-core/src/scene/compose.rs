//! The scene's composition: the image or a blank canvas with the enabled parts painted in.

use std::fmt;
use std::hash::{DefaultHasher, Hash, Hasher};
use std::sync::Mutex;

use serde::{Serialize, ser};

use super::painter::paint_part;
use super::pixels::ScenePixels;
use crate::model::{Project, SceneMode, Stencil};

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
		paint_part(&mut pixels, part, &project.simulation);
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
			scene.image_data.as_deref().map(image_hash).hash(&mut h);
		}
		(scene.width, scene.height, scene.refractive_index.to_bits()).hash(&mut h);
		let sim = &project.simulation;
		(sim.source_frequency_scale.to_bits(), sim.stencil == Stencil::FourthOrder).hash(&mut h);
		for part in project.parts.iter().filter(|p| p.base.enabled) {
			part.serialize(HashFeed(&mut h)).unwrap_or_else(|never| match never {});
		}
		Self(h.finish())
	}
}

/// A hash of an image scene's image bytes: equal keys decode alike. None for a canvas scene.
pub fn image_key(project: &Project) -> Option<u64> {
	let scene = &project.scene;
	(scene.mode == SceneMode::Image).then(|| scene.image_data.as_deref().map(image_hash)).flatten()
}

/// The image bytes' hash, remembered for the last image: an unchanged image is compared, not hashed again.
fn image_hash(bytes: &[u8]) -> u64 {
	static LAST: Mutex<Option<(Vec<u8>, u64)>> = Mutex::new(None);
	let mut last = LAST.lock().unwrap();
	if let Some((cached, hash)) = &*last
		&& cached[..] == *bytes
	{
		return *hash;
	}
	let mut h = DefaultHasher::new();
	bytes.hash(&mut h);
	let hash = h.finish();
	*last = Some((bytes.to_vec(), hash));
	hash
}

/// Feeds a value's serde form to a hasher, floats by their bits, without building the serialised text.
struct HashFeed<'a, H>(&'a mut H);

#[derive(Debug)]
enum Never {}

impl fmt::Display for Never {
	fn fmt(&self, _: &mut fmt::Formatter<'_>) -> fmt::Result {
		match *self {}
	}
}

impl std::error::Error for Never {}

impl ser::Error for Never {
	fn custom<T: fmt::Display>(message: T) -> Self {
		unreachable!("hashing does not fail: {message}")
	}
}

macro_rules! feed {
	($($method:ident($ty:ty) $(=> $conv:ident)?;)*) => {
		$(fn $method(self, v: $ty) -> Result<(), Never> {
			v$(.$conv())?.hash(self.0);
			Ok(())
		})*
	};
}

impl<'a, H: Hasher> ser::Serializer for HashFeed<'a, H> {
	type Ok = ();
	type Error = Never;
	type SerializeSeq = Self;
	type SerializeTuple = Self;
	type SerializeTupleStruct = Self;
	type SerializeTupleVariant = Self;
	type SerializeMap = Self;
	type SerializeStruct = Self;
	type SerializeStructVariant = Self;

	feed! {
		serialize_bool(bool);
		serialize_i8(i8);
		serialize_i16(i16);
		serialize_i32(i32);
		serialize_i64(i64);
		serialize_u8(u8);
		serialize_u16(u16);
		serialize_u32(u32);
		serialize_u64(u64);
		serialize_f32(f32) => to_bits;
		serialize_f64(f64) => to_bits;
		serialize_char(char);
		serialize_str(&str);
		serialize_bytes(&[u8]);
	}

	fn serialize_none(self) -> Result<(), Never> {
		0u8.hash(self.0);
		Ok(())
	}

	fn serialize_some<T: ?Sized + Serialize>(self, value: &T) -> Result<(), Never> {
		1u8.hash(self.0);
		value.serialize(self)
	}

	fn serialize_unit(self) -> Result<(), Never> {
		Ok(())
	}

	fn serialize_unit_struct(self, _: &'static str) -> Result<(), Never> {
		Ok(())
	}

	fn serialize_unit_variant(self, _: &'static str, index: u32, _: &'static str) -> Result<(), Never> {
		index.hash(self.0);
		Ok(())
	}

	fn serialize_newtype_struct<T: ?Sized + Serialize>(self, _: &'static str, value: &T) -> Result<(), Never> {
		value.serialize(self)
	}

	fn serialize_newtype_variant<T: ?Sized + Serialize>(
		self,
		_: &'static str,
		index: u32,
		_: &'static str,
		value: &T,
	) -> Result<(), Never> {
		index.hash(self.0);
		value.serialize(self)
	}

	fn serialize_seq(self, _: Option<usize>) -> Result<Self, Never> {
		Ok(self)
	}

	fn serialize_tuple(self, _: usize) -> Result<Self, Never> {
		Ok(self)
	}

	fn serialize_tuple_struct(self, _: &'static str, _: usize) -> Result<Self, Never> {
		Ok(self)
	}

	fn serialize_tuple_variant(self, _: &'static str, index: u32, _: &'static str, _: usize) -> Result<Self, Never> {
		index.hash(self.0);
		Ok(self)
	}

	fn serialize_map(self, _: Option<usize>) -> Result<Self, Never> {
		Ok(self)
	}

	fn serialize_struct(self, _: &'static str, _: usize) -> Result<Self, Never> {
		Ok(self)
	}

	fn serialize_struct_variant(self, _: &'static str, index: u32, _: &'static str, _: usize) -> Result<Self, Never> {
		index.hash(self.0);
		Ok(self)
	}
}

impl<H: Hasher> HashFeed<'_, H> {
	fn element<T: ?Sized + Serialize>(&mut self, value: &T) -> Result<(), Never> {
		value.serialize(HashFeed(&mut *self.0))
	}

	/// Ends a sequence, so that `[a], [b]` and `[a, b]` differ.
	fn end(self) -> Result<(), Never> {
		u8::MAX.hash(self.0);
		Ok(())
	}
}

macro_rules! compound {
	($($trait:ident :: $method:ident;)*) => {
		$(impl<H: Hasher> ser::$trait for HashFeed<'_, H> {
			type Ok = ();
			type Error = Never;

			fn $method<T: ?Sized + Serialize>(&mut self, value: &T) -> Result<(), Never> {
				self.element(value)
			}

			fn end(self) -> Result<(), Never> {
				HashFeed::end(self)
			}
		})*
	};
}

compound! {
	SerializeSeq::serialize_element;
	SerializeTuple::serialize_element;
	SerializeTupleStruct::serialize_field;
	SerializeTupleVariant::serialize_field;
}

impl<H: Hasher> ser::SerializeMap for HashFeed<'_, H> {
	type Ok = ();
	type Error = Never;

	fn serialize_key<T: ?Sized + Serialize>(&mut self, key: &T) -> Result<(), Never> {
		self.element(key)
	}

	fn serialize_value<T: ?Sized + Serialize>(&mut self, value: &T) -> Result<(), Never> {
		self.element(value)
	}

	fn end(self) -> Result<(), Never> {
		HashFeed::end(self)
	}
}

macro_rules! fields {
	($($trait:ident;)*) => {
		$(impl<H: Hasher> ser::$trait for HashFeed<'_, H> {
			type Ok = ();
			type Error = Never;

			fn serialize_field<T: ?Sized + Serialize>(&mut self, key: &'static str, value: &T) -> Result<(), Never> {
				key.hash(self.0);
				self.element(value)
			}

			fn end(self) -> Result<(), Never> {
				HashFeed::end(self)
			}
		})*
	};
}

fields! {
	SerializeStruct;
	SerializeStructVariant;
}
