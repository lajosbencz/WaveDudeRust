//! The tasks of [`waveduderust_app::jobs`] off the UI thread: the file pickers, a dropped file's read and the export
//! jobs, whose answers come back over egui-mineral's bus. Natively (`native`) each task runs on a thread of its own
//! with the platform's dialogs; in the browser (`web`) a picker uploads a file and a save downloads it.

use std::convert::Infallible;

use eframe::egui;
use egui_mineral::bus::Bus;
#[cfg(not(target_arch = "wasm32"))]
use waveduderust_app::export::session;
use waveduderust_app::jobs::{Answer, Done, Job, answered};
use waveduderust_app::shell::Host;
use waveduderust_app::strings::Strings;

#[cfg(not(target_arch = "wasm32"))]
mod native;
#[cfg(target_arch = "wasm32")]
mod web;

/// Work for a task: a job, or a file dropped on the window (an image becomes the scene's, another file opens).
enum Task {
	Job(Job),
	Dropped(egui::DroppedFileHandle),
}

/// The answers' channel, each answer with the document it was asked for.
type Answers = Bus<Infallible, (u64, Option<Answer>)>;

/// The tasks and the bus their answers come back on; the [`Host`] of the shell.
pub struct Jobs {
	/// Declared before the spawner: the answers' channel closes before the tasks are joined.
	bus: Answers,
	#[cfg(not(target_arch = "wasm32"))]
	spawner: native::ThreadSpawner,
	ctx: egui::Context,
}

impl Jobs {
	pub fn new(ctx: &egui::Context) -> Self {
		let (bus, _) = Bus::new(ctx);
		Self {
			bus,
			#[cfg(not(target_arch = "wasm32"))]
			spawner: native::ThreadSpawner::default(),
			ctx: ctx.clone(),
		}
	}

	/// Reads `file`, dropped on the window, for the document `document` with the strings `s`.
	pub fn drop_in(&self, file: egui::DroppedFileHandle, document: u64, s: Strings) {
		self.run(Task::Dropped(file), document, s);
	}

	/// The answers that arrived, those for another document than `document` dropped; finished tasks joined.
	pub fn take(&mut self, document: u64) -> Vec<Done> {
		#[cfg(not(target_arch = "wasm32"))]
		self.spawner.reap();
		answered(document, self.bus.drain().filter_map(|(of, answer)| answer.map(|a| (of, a))))
	}

	fn run(&self, task: Task, document: u64, s: Strings) {
		let reply = self.bus.sender();
		let reply = move |answer| drop(reply.send((document, answer)));
		#[cfg(not(target_arch = "wasm32"))]
		self.spawner.start(task, s, reply);
		#[cfg(target_arch = "wasm32")]
		web::run(task, s, reply);
	}
}

impl Host for Jobs {
	fn copy(&self, text: String) {
		self.ctx.copy_text(text);
	}

	fn start(&self, job: Job, document: u64, s: Strings) {
		self.run(Task::Job(job), document, s);
	}

	#[cfg(not(target_arch = "wasm32"))]
	fn export(&self, job: Box<session::Job>) {
		let reply = self.bus.sender();
		self.spawner.export(job, move |msg| drop(reply.send((0, Some(Answer::Export(msg))))));
	}
}
