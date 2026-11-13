//! The manual (docs/ui/70): the Markdown of `assets/manual.md`, its sections by heading, the figures of
//! `assets/manual` its `<figure name="..."/>` tags name, and the anchors its links point to.

/// The manual's Markdown.
pub const MANUAL: &str = include_str!("../assets/manual.md");

/// The figures the manual shows, by the name of its `<figure name="..."/>` tags.
pub mod figures {
	/// A figure: its name, the URI egui's image loaders know it by and its PNG.
	pub struct Figure {
		pub name: &'static str,
		pub uri: &'static str,
		pub png: &'static [u8],
	}

	impl Figure {
		/// Width and height, from the PNG's header.
		pub fn size(&self) -> [u32; 2] {
			let at = |i: usize| u32::from_be_bytes(self.png[i..i + 4].try_into().expect("four bytes"));
			[at(16), at(20)]
		}
	}

	macro_rules! figures {
		($($name:literal),* $(,)?) => {
			/// Every figure.
			pub const ALL: &[Figure] = &[$(Figure {
				name: $name,
				uri: concat!("bytes://manual/", $name, ".png"),
				png: include_bytes!(concat!("../assets/manual/", $name, ".png")),
			}),*];
		};
	}
	figures!(
		"main-window",
		"project-tree",
		"preview",
		"timeline",
		"menu-file",
		"menu-file-export",
		"menu-edit",
		"menu-view",
		"menu-simulation",
		"menu-help",
		"menu-add-part",
		"menu-add-probe",
		"properties-project",
		"properties-scene",
		"properties-simulation",
		"properties-display",
		"properties-source",
		"properties-composite-source",
		"properties-lens",
		"properties-fresnel-lens",
		"properties-polygon",
		"properties-rectangle",
		"properties-ellipse",
		"properties-triangle",
		"properties-outline",
		"properties-fibre",
		"properties-bragg-reflector",
		"properties-grin-lens",
		"properties-zone-plate",
		"properties-diffuser",
		"properties-interference-filter",
		"properties-simple-grating",
		"properties-facetted-grating",
		"properties-meta-lens",
		"properties-meta-prism",
		"properties-meta-axicon",
		"properties-xor-gate",
		"properties-probe",
		"properties-line-probe",
		"properties-counting-detector",
		"properties-imaging-detector",
		"properties-colour-scheme",
	);

	pub fn get(name: &str) -> Option<&'static Figure> {
		ALL.iter().find(|f| f.name == name)
	}
}

/// One heading and the text up to the next heading.
#[derive(Clone, Debug, PartialEq)]
pub struct Section {
	pub level: usize,
	pub title: String,
	pub body: String,
}

/// The manual split at its headings; the title (`#`) is no row.
pub fn sections(text: &str) -> Vec<Section> {
	let mut out: Vec<Section> = Vec::new();
	for line in text.lines() {
		let level = line.chars().take_while(|&c| c == '#').count();
		if level > 0 && line[level..].starts_with(' ') {
			out.push(Section { level, title: line[level..].trim().to_owned(), body: format!("{line}\n") });
		} else if let Some(last) = out.last_mut() {
			last.body.push_str(line);
			last.body.push('\n');
		}
	}
	out.retain(|s| s.level > 1 || !s.body.lines().skip(1).all(str::is_empty));
	out
}

/// The figure names a `<figure name="..."/>` tag names in `html`.
pub fn figure_name(html: &str) -> Option<&str> {
	let rest = html.trim().strip_prefix("<figure")?;
	let start = rest.find("name=\"")? + 6;
	let end = rest[start..].find('"')? + start;
	Some(&rest[start..end])
}

/// GitHub's anchor of a heading: lower case, spaces as hyphens, punctuation dropped.
pub fn anchor(title: &str) -> String {
	title
		.chars()
		.filter_map(|c| match c {
			' ' | '-' => Some('-'),
			c if c.is_alphanumeric() => Some(c.to_ascii_lowercase()),
			_ => None,
		})
		.collect()
}

#[cfg(test)]
mod tests {
	use super::*;

	#[test]
	fn manual_splits_at_its_chapters() {
		let all = sections(MANUAL);
		let titles: Vec<&str> = all.iter().filter(|s| s.level == 2).map(|s| s.title.as_str()).collect();
		assert_eq!(titles.first(), Some(&"The main window"));
		assert!(titles.contains(&"The examples"));
		assert!(all.iter().all(|s| s.body.starts_with('#')));
	}

	#[test]
	fn every_figure_tag_names_a_bundled_figure_and_every_anchor_a_heading() {
		let all = sections(MANUAL);
		let anchors: Vec<String> = all.iter().map(|s| anchor(&s.title)).collect();
		for line in MANUAL.lines() {
			let mut rest = line;
			while let Some(start) = rest.find("<figure") {
				let tag = &rest[start..];
				let name = figure_name(tag).expect("a figure tag names its figure");
				assert!(figures::get(name).is_some(), "no figure '{name}'");
				rest = &tag[1..];
			}
			let mut rest = line;
			while let Some(start) = rest.find("](#") {
				let target = &rest[start + 3..];
				let end = target.find(')').expect("a link closes");
				assert!(anchors.contains(&target[..end].to_owned()), "no heading for #{}", &target[..end]);
				rest = &target[end..];
			}
		}
	}

	#[test]
	fn a_figure_s_size_is_its_png_s() {
		assert_eq!(figures::get("main-window").unwrap().size(), [1400, 900]);
	}

	#[test]
	fn anchors_follow_github() {
		assert_eq!(anchor("The File menu"), "the-file-menu");
		assert_eq!(anchor("Lens/Mirror"), "lensmirror");
		assert_eq!(anchor("XOR gate"), "xor-gate");
	}
}
