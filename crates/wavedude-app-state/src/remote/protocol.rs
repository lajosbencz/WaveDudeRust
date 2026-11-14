//! The messages between the page and the run worker. A message is postcard; the planes of a frame or a fit and
//! the bytes of a scene image travel beside it as its buffer.

use serde::{Deserialize, Serialize};
use wavedude_core::engine::FrameShape;
use wavedude_core::engine::probes::ProbeTrace;
use wavedude_core::model::Layer;
use wavedude_core::run::{Goal, RunError, StoreLayout};
use wavedude_core::spec::RunSpecError;

/// From the page to the run worker.
#[derive(Debug, Serialize, Deserialize)]
pub enum ToWorker {
	/// The first message: whether the engines run on the GPU.
	Init { use_gpu: bool },
	/// The scene image of key `key` (`image_key`), its bytes the buffer; posted before the first goal naming it.
	Image { key: u64 },
	/// The goal, its project without the image bytes; `image` is the key of the image to put back.
	Goal { goal: Goal, image: Option<u64> },
	/// The engine is built anew, on the GPU when `use_gpu`.
	Rebuild { use_gpu: bool },
	/// The frames go for a new store (`RunWorker::discard_frames`).
	Discard,
	/// The page holds frame `frame` of `generation` of store `store`, or why it could not keep it.
	Ack { store: u64, generation: u64, frame: usize, result: Result<(), String> },
}

/// From the run worker to the page.
#[derive(Debug, Serialize, Deserialize)]
pub enum ToPage {
	/// The worker started: its GPU adapter's name, or why it has none.
	Ready {
		gpu: Result<String, String>,
	},
	/// An engine was built: its label and, when the CPU's runs where the GPU's was asked for, why.
	EngineBuilt {
		name: String,
		fallback: Option<String>,
	},
	/// A store of the run, posted before its frames and the status naming it.
	StoreOpened {
		id: u64,
		layout: Layout,
	},
	/// Frame `frame` of `generation` of store `store`, its planes in layer order the buffer.
	Frame {
		store: u64,
		generation: u64,
		frame: usize,
	},
	Status(RunStatus),
	/// The traces recorded since the last, in probe order. With `held`, the traces start over from `tails` and
	/// the traces held with the frame kept through the last clear are `held`.
	Traces {
		held: Option<Vec<Trace>>,
		tails: Vec<Trace>,
	},
	/// The fit frame served, its field and then its intensity plane, when captured, the buffer.
	Fit {
		id: u64,
		frame: u32,
		step: u32,
		shape: FrameShape,
	},
	/// The worker died of this.
	Crashed(String),
}

/// A [`StoreLayout`] on the wire: the layers are the field, with the intensity when `intensity`.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct Layout {
	pub shape: FrameShape,
	pub intensity: bool,
	pub capacity: usize,
	pub capture_every: u32,
}

const FIELD: &[Layer] = &[Layer::Field];
const BOTH: &[Layer] = &[Layer::Field, Layer::Intensity];

impl From<&StoreLayout> for Layout {
	fn from(layout: &StoreLayout) -> Self {
		Self {
			shape: layout.shape,
			intensity: layout.layers.contains(&Layer::Intensity),
			capacity: layout.capacity,
			capture_every: layout.capture_every,
		}
	}
}

impl From<Layout> for StoreLayout {
	fn from(layout: Layout) -> Self {
		Self {
			shape: layout.shape,
			layers: if layout.intensity { BOTH } else { FIELD },
			capacity: layout.capacity,
			capture_every: layout.capture_every,
		}
	}
}

/// What the run published, as `wavedude_core::run::Status` without its shared parts.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct RunStatus {
	pub adopted: u64,
	pub state: State,
	pub steps_done: u32,
	pub traces_steps: u32,
	/// The id of the store holding the run's frames.
	pub store: Option<u64>,
	/// The store's generation.
	pub generation: u64,
}

/// `wavedude_core::run::WorkState` on the wire.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub enum State {
	#[default]
	Idle,
	Stepping {
		target: u32,
	},
	Fitting,
	Paused,
	/// The run's `id`th failure.
	Failed {
		id: u64,
		failure: Failure,
	},
}

/// A [`RunError`] on the wire: what the page tells apart, else the message.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub enum Failure {
	Spec(RunSpecError),
	Other(String),
}

impl From<&RunError> for Failure {
	fn from(e: &RunError) -> Self {
		match e {
			RunError::Spec(e) => Self::Spec(e.clone()),
			RunError::Failed(e) => Self::from(&**e),
			e => Self::Other(e.to_string()),
		}
	}
}

impl From<Failure> for RunError {
	fn from(failure: Failure) -> Self {
		match failure {
			Failure::Spec(e) => Self::Spec(e),
			Failure::Other(message) => Self::Worker(message),
		}
	}
}

/// A probe's readings past a point of its [`ProbeTrace`], with the trace's length and steps sampled.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Trace {
	pub series: Vec<f32>,
	pub profiles: Vec<Vec<f32>>,
	pub length: f64,
	pub steps_sampled: u64,
}

impl Trace {
	/// What `trace` holds past its first `series` values and `profiles` profiles.
	pub fn tail(trace: &ProbeTrace, series: usize, profiles: usize) -> Self {
		Self {
			series: trace.series[series..].to_vec(),
			profiles: trace.profiles.iter().skip(profiles).map(<[f32]>::to_vec).collect(),
			length: trace.length,
			steps_sampled: trace.steps_sampled,
		}
	}

	/// Appends the readings to `trace`.
	pub fn append_to(self, trace: &mut ProbeTrace) {
		trace.series.extend(self.series);
		for profile in self.profiles {
			trace.profiles.push(profile.into_iter());
		}
		trace.length = self.length;
		trace.steps_sampled = self.steps_sampled;
	}
}
