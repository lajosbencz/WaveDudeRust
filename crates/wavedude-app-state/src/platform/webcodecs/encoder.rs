//! The `VideoEncoder` behind [`WebCodecsFactory`] and the probe of [`support`].

use std::cell::RefCell;
use std::collections::HashMap;
use std::io;
use std::rc::Rc;
use std::task::Poll;

use js_sys::{Object, Reflect};
use wasm_bindgen_futures::spawn_local;
use wavedude_core::run::Token;
use wavedude_export::h264::{EncoderSpec, H264Encoder, H264Factory, Profile, Yuv420};
use wavedude_export::{ExportError, Result};
use web_sys::wasm_bindgen::closure::Closure;
use web_sys::wasm_bindgen::{JsCast, JsValue};
use web_sys::{
	CodecState, EncodedVideoChunk, EncodedVideoChunkType, HardwareAcceleration, LatencyMode, VideoColorPrimaries,
	VideoColorSpaceInit, VideoEncoder, VideoEncoderBitrateMode, VideoEncoderConfig, VideoEncoderEncodeOptions,
	VideoEncoderInit, VideoFrame, VideoFrameBufferInit, VideoMatrixCoefficients, VideoPixelFormat,
	VideoTransferCharacteristics,
};

use super::{Chunks, codec, pack_i420, timestamp};
use crate::platform::web::describe;
use crate::wake;

/// What the browser encodes at a picture size and rate.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Support {
	/// The browser has no WebCodecs `VideoEncoder`.
	Unavailable,
	/// The browser is being asked.
	Probing,
	/// The profiles it encodes, maybe none.
	Profiles(Vec<Profile>),
}

/// A picture size, rate and profile.
type Key = (u32, u32, u32, Profile);

thread_local! {
	/// Whether the browser encodes each key asked about; `None` while it is being asked.
	static PROBES: RefCell<HashMap<Key, Option<bool>>> = RefCell::default();
}

fn available() -> bool {
	Reflect::has(&js_sys::global(), &"VideoEncoder".into()).unwrap_or(false)
}

fn failed(e: &JsValue) -> ExportError {
	ExportError::H264(format!("the browser's encoder: {}", describe(e)))
}

/// What the browser encodes at `width x height` (padded to even) and `fps`; asks it the first time, waking the
/// window once each answer is in.
pub fn support(width: u32, height: u32, fps: u32) -> Support {
	if !available() {
		return Support::Unavailable;
	}
	let (width, height) = (width.next_multiple_of(2), height.next_multiple_of(2));
	PROBES.with_borrow_mut(|probes| {
		let mut profiles = Vec::new();
		let mut probing = false;
		for profile in Profile::ALL {
			let key = (width, height, fps, profile);
			let answer = *probes.entry(key).or_insert_with(|| match config(width, height, fps, profile) {
				Ok(config) => {
					probe(key, config);
					None
				}
				Err(_) => Some(false),
			});
			match answer {
				Some(true) => profiles.push(profile),
				Some(false) => {}
				None => probing = true,
			}
		}
		if probing { Support::Probing } else { Support::Profiles(profiles) }
	})
}

/// Asks the browser whether it encodes `config`, the answer stored under `key`.
fn probe(key: Key, config: VideoEncoderConfig) {
	spawn_local(async move {
		let supported = match VideoEncoder::is_config_supported(&config).await {
			// a browser that ignores `avc` would write length-prefixed NAL units, which the muxer cannot read
			Ok(support) => support.get_supported().unwrap_or(false) && keeps_annexb(&support),
			Err(e) => {
				tracing::debug!(?key, "the browser refused to check an H.264 configuration: {}", describe(&e));
				false
			}
		};
		tracing::debug!(?key, supported, "H.264 configuration checked");
		PROBES.with_borrow_mut(|probes| probes.insert(key, Some(supported)));
		wake::wake();
	});
}

/// Whether the configuration the browser answered with still asks for Annex B output.
fn keeps_annexb(support: &web_sys::VideoEncoderSupport) -> bool {
	let format = support
		.get_config()
		.and_then(|config| Reflect::get(&config, &"avc".into()).ok())
		.and_then(|avc| Reflect::get(&avc, &"format".into()).ok())
		.and_then(|format| format.as_string());
	format.as_deref() == Some("annexb")
}

/// The encoder configuration of `profile` at `width x height` and `fps`, without a bitrate.
fn config(width: u32, height: u32, fps: u32, profile: Profile) -> Result<VideoEncoderConfig> {
	let codec = codec(profile, width, height, fps).ok_or(ExportError::Size {
		format: "MP4",
		width,
		height,
		limit: "within H.264 level 6.2 at its frame rate",
	})?;
	let config = VideoEncoderConfig::new(&codec, height, width);
	config.set_framerate(f64::from(fps));
	config.set_bitrate_mode(VideoEncoderBitrateMode::Variable);
	config.set_latency_mode(LatencyMode::Quality);
	config.set_hardware_acceleration(HardwareAcceleration::NoPreference);
	// web-sys has no AvcEncoderConfig.
	let avc = Object::new();
	Reflect::set(&avc, &"format".into(), &"annexb".into()).map_err(|e| failed(&e))?;
	Reflect::set(&config, &"avc".into(), &avc).map_err(|e| failed(&e))?;
	Ok(config)
}

/// Opens WebCodecs encoders in the profiles the browser encodes at one picture size and rate.
#[derive(Clone, Debug)]
pub struct WebCodecsFactory {
	profiles: Vec<Profile>,
}

impl WebCodecsFactory {
	/// Encoders of `width x height` (padded to even) at `fps`, in the profiles [`support`] found; none while it
	/// asks.
	pub fn new(width: u32, height: u32, fps: u32) -> Self {
		let profiles = match support(width, height, fps) {
			Support::Profiles(profiles) => profiles,
			Support::Unavailable | Support::Probing => Vec::new(),
		};
		Self { profiles }
	}
}

impl H264Factory for WebCodecsFactory {
	fn profiles(&self) -> &[Profile] {
		&self.profiles
	}

	fn open(&self, spec: &EncoderSpec) -> Result<Box<dyn H264Encoder>> {
		if !self.profiles.contains(&spec.profile) {
			return Err(ExportError::H264(format!("this browser does not encode {:?}", spec.profile)));
		}
		let config = config(spec.width, spec.height, spec.fps, spec.profile)?;
		config.set_bitrate(spec.bitrate);
		let chunks = Rc::new(RefCell::new(Chunks::new(spec.fps)));
		let queue = chunks.clone();
		let output = Closure::<dyn FnMut(EncodedVideoChunk)>::new(move |chunk: EncodedVideoChunk| {
			let mut data = vec![0; chunk.byte_length() as usize];
			match chunk.copy_to_with_u8_slice(&mut data) {
				Ok(()) => {
					let sync = chunk.type_() == EncodedVideoChunkType::Key;
					queue.borrow_mut().push(chunk.timestamp() as i64, data, sync);
				}
				Err(e) => queue.borrow_mut().fail(format!("the browser's encoder: {}", describe(&e))),
			}
			wake::wake();
		});
		let queue = chunks.clone();
		let error = Closure::<dyn FnMut(JsValue)>::new(move |e: JsValue| {
			queue.borrow_mut().fail(format!("the browser's encoder: {}", describe(&e)));
			wake::wake();
		});
		let init = VideoEncoderInit::new(error.as_ref().unchecked_ref(), output.as_ref().unchecked_ref());
		let color = VideoColorSpaceInit::new();
		color.set_primaries(Some(VideoColorPrimaries::Smpte170m));
		color.set_transfer(Some(VideoTransferCharacteristics::Smpte170m));
		color.set_matrix(Some(VideoMatrixCoefficients::Smpte170m));
		color.set_full_range(Some(false));
		let encoder = WebCodecsEncoder {
			encoder: VideoEncoder::new(&init).map_err(|e| failed(&e))?,
			chunks,
			_output: output,
			_error: error,
			fps: spec.fps,
			color,
			planes: Vec::new(),
			flushed: None,
		};
		encoder.encoder.configure(&config).map_err(|e| failed(&e))?;
		tracing::debug!(codec = config.get_codec(), bps = spec.bitrate, "WebCodecs encoder configured");
		Ok(Box::new(encoder))
	}
}

/// A `VideoEncoder`, closed when dropped.
struct WebCodecsEncoder {
	encoder: VideoEncoder,
	chunks: Rc<RefCell<Chunks>>,
	/// The encoder's callbacks, which it calls until it is closed.
	_output: Closure<dyn FnMut(EncodedVideoChunk)>,
	_error: Closure<dyn FnMut(JsValue)>,
	fps: u32,
	/// BT.601 studio range, as `yuv` converts.
	color: VideoColorSpaceInit,
	/// The I420 picture handed to the browser, its allocation reused.
	planes: Vec<u8>,
	/// Done once the encoder's flush settled.
	flushed: Option<Token<()>>,
}

impl H264Encoder for WebCodecsEncoder {
	fn poll_ready(&mut self) -> Result<Poll<()>> {
		Ok(if self.chunks.borrow().ready()? { Poll::Ready(()) } else { Poll::Pending })
	}

	fn submit(&mut self, picture: &Yuv420) -> Result<()> {
		let (index, key) = {
			let mut chunks = self.chunks.borrow_mut();
			chunks.check()?;
			chunks.submit()
		};
		pack_i420(picture, &mut self.planes);
		let start = timestamp(index, self.fps);
		let init = VideoFrameBufferInit::new(picture.height as u32, picture.width as u32, VideoPixelFormat::I420, 0);
		init.set_timestamp_f64(start as f64);
		init.set_duration_f64((timestamp(index + 1, self.fps) - start) as f64);
		init.set_color_space(&self.color);
		let frame = VideoFrame::new_with_u8_slice_and_video_frame_buffer_init(&mut self.planes, &init)
			.map_err(|e| failed(&e))?;
		let options = VideoEncoderEncodeOptions::new();
		options.set_key_frame(key);
		let encoded = self.encoder.encode_with_options(&frame, &options);
		frame.close();
		encoded.map_err(|e| failed(&e))
	}

	fn drain(&mut self, out: &mut dyn FnMut(&[u8], bool) -> Result<()>) -> Result<()> {
		self.chunks.borrow_mut().drain(out)
	}

	fn start_flush(&mut self) -> Result<()> {
		self.chunks.borrow().check()?;
		let flushed = Token::default();
		let (task, promise) = (flushed.clone(), self.encoder.flush());
		spawn_local(async move {
			task.complete(promise.await.map(drop).map_err(|e| io::Error::other(describe(&e))));
			wake::wake();
		});
		self.flushed = Some(flushed);
		Ok(())
	}

	fn poll_flushed(&mut self) -> Result<Poll<()>> {
		let flushed = self.flushed.as_ref().ok_or_else(|| ExportError::H264("the encoder was not flushed".into()))?;
		if !flushed.is_done() {
			return self.chunks.borrow().check().map(|()| Poll::Pending);
		}
		self.chunks.borrow().check()?;
		if let Some(Err(e)) = flushed.take() {
			return Err(ExportError::H264(format!("the browser's encoder did not flush: {e}")));
		}
		self.chunks.borrow().flushed().map(Poll::Ready)
	}
}

impl Drop for WebCodecsEncoder {
	fn drop(&mut self) {
		if self.encoder.state() != CodecState::Closed
			&& let Err(e) = self.encoder.close()
		{
			tracing::warn!("the browser's encoder could not be closed: {}", describe(&e));
		}
	}
}
