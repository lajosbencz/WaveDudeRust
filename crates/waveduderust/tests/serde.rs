#![cfg(all(feature = "serde", feature = "run"))]
//! A run's goal through postcard, as it crosses to the browser's run worker (decision 0024).

use std::path::{Path, PathBuf};

use waveduderust::io::wdproj;
use waveduderust::model::Project;
use waveduderust::model::normalize::Normalize;
use waveduderust::quality::{Brightness, PreviewQuality};
use waveduderust::run::{FitRequest, Goal, RamCache, Wanted};
use waveduderust::unstable::spec::RunRequest;

/// The example projects: the core's fixtures and the repository's `examples`.
fn examples() -> Vec<PathBuf> {
	let manifest = Path::new(env!("CARGO_MANIFEST_DIR"));
	let mut dirs = vec![manifest.join("tests/fixtures/examples")];
	let mut files = Vec::new();
	dirs.extend(
		std::fs::read_dir(manifest.join("../../examples")).unwrap().map(|e| e.unwrap().path()).filter(|p| p.is_dir()),
	);
	for dir in dirs {
		files.extend(
			std::fs::read_dir(dir)
				.unwrap()
				.map(|e| e.unwrap().path())
				.filter(|p| p.extension().is_some_and(|e| e == "wdproj")),
		);
	}
	files.sort();
	assert!(!files.is_empty(), "no example projects found");
	files
}

fn round_trip(project: &Project) -> Project {
	let goal = Goal {
		run: Some(RunRequest::new(project, PreviewQuality::FULL, Brightness::default(), false)),
		..Goal::default()
	};
	let back: Goal = postcard::from_bytes(&postcard::to_allocvec(&goal).unwrap()).unwrap();
	(*back.run.unwrap().project).clone()
}

#[test]
fn every_examples_goal_round_trips_through_postcard() {
	for path in examples() {
		let (mut project, view) = wdproj::read_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
		project.normalize();
		let quality = PreviewQuality { intensity_root: false, ..PreviewQuality::DEFAULT };
		let goal = Goal {
			revision: 7,
			run: Some(RunRequest::new(&project, quality, (&view).into(), true)),
			position: 12,
			wanted: Wanted::All,
			paused: true,
			fit: Some(FitRequest { id: 3, frame: 9 }),
			ram_cache: RamCache::Percent(40),
		};
		let bytes = postcard::to_allocvec(&goal).unwrap();
		let back: Goal = postcard::from_bytes(&bytes).unwrap();
		let run = back.run.unwrap();
		assert_eq!(*run.project, project, "{}", path.display());
		assert_eq!((run.quality, run.brightness, run.track_intensity), (quality, (&view).into(), true));
		assert_eq!(
			(back.revision, back.position, back.wanted, back.paused, back.fit, back.ram_cache),
			(goal.revision, goal.position, goal.wanted, goal.paused, goal.fit, goal.ram_cache)
		);
	}
}

#[test]
fn the_scenes_image_bytes_stay_behind() {
	let mut project = Project::default();
	project.scene.image_data = Some(vec![1, 2, 3].into());
	project.scene.image_size = (4, 5);
	let back = round_trip(&project);
	assert_eq!(back.scene.image_data, None);
	project.scene.image_data = None;
	assert_eq!(back, project, "the rest of the project, the image's size with it");
}
