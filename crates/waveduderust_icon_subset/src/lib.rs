//! Build helper: subsets `egui_material_icons`' fonts to the `ICON_*` constants named by every package that
//! depends on it, and writes them to `OUT_DIR` under their file names. Call from a `build.rs`.

#![allow(clippy::print_stdout)] // cargo build-script protocol

use std::{
	collections::{BTreeSet, HashMap, HashSet},
	env,
	error::Error,
	fs,
	path::{Path, PathBuf},
	process::Command,
};

use allsorts::{
	Font,
	binary::read::ReadScope,
	font::MatchingPresentation,
	font_data::FontData,
	subset::{CmapTarget, SubsetProfile, subset},
};
use serde_json::Value;

type Res<T> = Result<T, Box<dyn Error>>;

const ICONS: &str = "egui_material_icons";
// Names its own glyphs (`icon_set.rs`) from its own `ICON_*` table, without depending on `ICONS`.
const KIT: &str = "egui_mineral_material";
const SOURCE_KINDS: [&str; 5] = ["lib", "rlib", "bin", "proc-macro", "cdylib"];

/// Subsets the font files `fonts` (names in the `egui_material_icons` package) into `OUT_DIR`.
pub fn subset_icon_fonts(fonts: &[&str]) -> Res<()> {
	let meta = metadata()?;
	let packages: HashMap<&str, &Value> = meta["packages"]
		.as_array()
		.ok_or("metadata: no packages")?
		.iter()
		.filter_map(|p| Some((p["id"].as_str()?, p)))
		.collect();
	let icons_id = packages
		.iter()
		.find(|(_, p)| p["name"] == ICONS)
		.map(|(id, _)| *id)
		.ok_or("metadata: no egui_material_icons")?;
	let mut names = HashSet::new();
	let kit_id = packages.iter().find(|(_, p)| p["name"] == KIT).map(|(id, _)| *id);
	let roots: Vec<&str> = [Some(icons_id), kit_id].into_iter().flatten().collect();
	for dir in source_dirs(&meta, &packages, &roots, icons_id)? {
		println!("cargo::rerun-if-changed={}", dir.display());
		scan(&dir, &mut names)?;
	}
	let workspace = meta["workspace_root"].as_str().ok_or("metadata: no workspace_root")?;
	println!("cargo::rerun-if-changed={}", Path::new(workspace).join("Cargo.lock").display());

	let icons_dir = packages[icons_id]["manifest_path"]
		.as_str()
		.and_then(|p| Path::new(p).parent())
		.ok_or("metadata: no egui_material_icons manifest")?;
	let icons_rs = fs::read_to_string(icons_dir.join("src/icons.rs"))?;
	let codepoints = codepoints(&icons_rs);
	let chars: BTreeSet<char> = names.iter().filter_map(|n| codepoints.get(n.as_str()).copied()).collect();
	let out = PathBuf::from(env::var("OUT_DIR")?);
	for name in fonts {
		fs::write(out.join(name), subset_font(&fs::read(icons_dir.join(name))?, &chars)?)?;
	}
	Ok(())
}

fn metadata() -> Res<Value> {
	let out = Command::new(env::var("CARGO")?)
		.args(["metadata", "--format-version", "1", "--offline", "--filter-platform", &env::var("TARGET")?])
		.arg("--manifest-path")
		.arg(Path::new(&env::var("CARGO_MANIFEST_DIR")?).join("Cargo.toml"))
		.output()?;
	if !out.status.success() {
		return Err(format!("cargo metadata: {}", String::from_utf8_lossy(&out.stderr)).into());
	}
	Ok(serde_json::from_slice(&out.stdout)?)
}

/// Source directories of `roots` and of the packages that depend, directly or not, on one of them, except `table`.
fn source_dirs<'a>(
	meta: &'a Value,
	packages: &HashMap<&str, &Value>,
	roots: &[&'a str],
	table: &str,
) -> Res<BTreeSet<PathBuf>> {
	let nodes = meta["resolve"]["nodes"].as_array().ok_or("metadata: no resolve")?;
	let edges: Vec<(&str, Vec<&str>)> = nodes
		.iter()
		.filter_map(|n| {
			let deps = n["deps"].as_array()?.iter().filter_map(|d| d["pkg"].as_str()).collect();
			Some((n["id"].as_str()?, deps))
		})
		.collect();
	let mut reached: HashSet<&str> = roots.iter().copied().collect();
	loop {
		let before = reached.len();
		for (id, deps) in &edges {
			if deps.iter().any(|d| reached.contains(d)) {
				reached.insert(id);
			}
		}
		if reached.len() == before {
			break;
		}
	}
	reached.remove(table);
	let mut dirs = BTreeSet::new();
	for id in reached {
		let targets = packages.get(id).and_then(|p| p["targets"].as_array()).ok_or("metadata: no targets")?;
		for t in targets {
			let kinds = t["kind"].as_array().ok_or("metadata: no target kind")?;
			if kinds.iter().any(|k| k.as_str().is_some_and(|k| SOURCE_KINDS.contains(&k)))
				&& let Some(dir) = t["src_path"].as_str().and_then(|p| Path::new(p).parent())
			{
				dirs.insert(dir.to_path_buf());
			}
		}
	}
	Ok(dirs)
}

fn scan(dir: &Path, names: &mut HashSet<String>) -> Res<()> {
	for entry in fs::read_dir(dir)? {
		let path = entry?.path();
		if path.is_dir() {
			scan(&path, names)?;
		} else if path.extension().is_some_and(|e| e == "rs") {
			let src = fs::read_to_string(&path)?;
			names.extend(
				src.split(|c: char| !(c.is_ascii_alphanumeric() || c == '_'))
					.filter(|t| t.starts_with("ICON_"))
					.map(str::to_owned),
			);
		}
	}
	Ok(())
}

// `pub const ICON_X: crate::MaterialIcon = crate::MaterialIcon::new("\u{e951}");` lines of `icons.rs`.
fn codepoints(icons_rs: &str) -> HashMap<&str, char> {
	icons_rs
		.lines()
		.filter_map(|l| {
			let (name, rest) = l.strip_prefix("pub const ")?.split_once(':')?;
			let (_, hex) = rest.split_once("\\u{")?;
			let (hex, _) = hex.split_once('}')?;
			Some((name, char::from_u32(u32::from_str_radix(hex, 16).ok()?)?))
		})
		.collect()
}

fn subset_font(ttf: &[u8], chars: &BTreeSet<char>) -> Res<Vec<u8>> {
	let data = ReadScope::new(ttf).read::<FontData<'_>>()?;
	let mut font = Font::new(data.table_provider(0)?)?;
	// Glyph 0 (.notdef) is mandatory.
	let mut glyphs = BTreeSet::from([0]);
	for c in chars {
		glyphs.insert(font.lookup_glyph_index(*c, MatchingPresentation::NotRequired, None).0);
	}
	let glyphs: Vec<u16> = glyphs.into_iter().collect();
	Ok(subset(&font.font_table_provider, &glyphs, &SubsetProfile::Minimal, CmapTarget::Unicode)?)
}
