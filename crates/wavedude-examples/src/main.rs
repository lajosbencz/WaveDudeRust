//! `wavedude-examples <out-dir>`: writes every example file and `catalog.json`, which the web demo reads
//! (topics, examples and quality tiers in the application's order, with the application's English titles).

use std::collections::HashMap;
use std::path::PathBuf;

use anyhow::{Context, Result, bail};
use serde_json::{Value, json};
use wavedude_core::quality::PreviewTier;
use wavedude_examples::{Topic, all};

const STRINGS: &str = include_str!("../../wavedude-app-state/assets/strings/en.json");

fn strings() -> Result<HashMap<String, String>> {
	serde_json::from_str(STRINGS).context("the English string table is a flat object of strings")
}

fn catalog(strings: &HashMap<String, String>) -> Result<Value> {
	let title = |key: &str| strings.get(key).with_context(|| format!("the English string table lacks '{key}'"));
	let topics = Topic::ALL
		.iter()
		.map(|t| Ok(json!({ "id": format!("{t:?}"), "title": title(t.title_key())? })))
		.collect::<Result<Vec<_>>>()?;
	let examples: Vec<Value> = all()
		.iter()
		.map(|e| {
			json!({
				"file": e.file_name,
				"title": e.title,
				"description": e.description,
				"topic": format!("{:?}", e.topic),
			})
		})
		.collect();
	let tiers = PreviewTier::ALL
		.iter()
		.map(|t| Ok(json!({ "id": t.name(), "title": title(t.title_key())? })))
		.collect::<Result<Vec<_>>>()?;
	Ok(json!({ "topics": topics, "examples": examples, "tiers": tiers }))
}

fn main() -> Result<()> {
	let mut args = std::env::args().skip(1);
	let (Some(out), None) = (args.next().map(PathBuf::from), args.next()) else {
		bail!("usage: wavedude-examples <out-dir>");
	};
	std::fs::create_dir_all(&out).with_context(|| format!("creating {}", out.display()))?;
	for example in all() {
		let path = out.join(example.file_name);
		std::fs::write(&path, example.bytes()).with_context(|| format!("writing {}", path.display()))?;
	}
	let path = out.join("catalog.json");
	let text = serde_json::to_string_pretty(&catalog(&strings()?)?)?;
	std::fs::write(&path, text).with_context(|| format!("writing {}", path.display()))?;
	Ok(())
}

#[cfg(test)]
mod tests {
	use super::*;

	#[test]
	fn the_english_table_has_every_key_the_catalog_names() {
		let strings = strings().unwrap();
		for key in Topic::ALL.iter().map(|t| t.title_key()).chain(PreviewTier::ALL.iter().map(|t| t.title_key())) {
			assert!(strings.contains_key(key), "en.json lacks '{key}'");
		}
	}

	#[test]
	fn the_catalog_lists_the_topics_examples_and_tiers_in_order() {
		let catalog = catalog(&strings().unwrap()).unwrap();
		assert_eq!(catalog["topics"].as_array().unwrap().len(), 6);
		assert_eq!(catalog["topics"][0], json!({ "id": "Sources", "title": "Sources and coherence" }));
		assert_eq!(catalog["examples"].as_array().unwrap().len(), all().len());
		assert_eq!(catalog["examples"][0]["file"], "Example01_Source_Mirror_Probe.wdproj");
		assert_eq!(catalog["examples"][0]["topic"], "Sources");
		assert_eq!(
			catalog["tiers"],
			json!([
				{ "id": "Low", "title": "Low" },
				{ "id": "Middle", "title": "Standard" },
				{ "id": "High", "title": "High" },
			])
		);
	}
}
