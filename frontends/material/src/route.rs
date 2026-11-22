//! The workspace and the pages over it (DESIGN.md, Overlay pages).

use egui_mineral_material::prelude::{MaterialIcon, icons};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, egui_mineral::nav::Route)]
pub(crate) enum Route {
	#[route("/")]
	Workspace,
	#[route("/examples")]
	Examples,
	/// `section` indexes the shown settings sections; `None` is the section list where it is a page of its own.
	#[route("/settings/{section?}")]
	Settings { section: Option<usize> },
	#[route("/about")]
	About,
}

impl Route {
	/// The pages over the workspace, as the palette lists them.
	pub(crate) const PAGES: [Self; 3] = [Self::Examples, Self::Settings { section: None }, Self::About];

	pub(crate) fn icon(self) -> MaterialIcon {
		match self {
			Self::Workspace => icons::ICON_WAVES,
			Self::Examples => icons::ICON_LIST_ALT,
			Self::Settings { .. } => icons::ICON_SETTINGS,
			Self::About => icons::ICON_INFO,
		}
	}

	pub(crate) fn title_key(self) -> &'static str {
		match self {
			Self::Workspace => "Title",
			Self::Examples => "ExamplesTitle",
			Self::Settings { .. } => "SettingsTitle",
			Self::About => "NavAbout",
		}
	}

	/// The same page, whatever its section.
	pub(crate) fn same_page(self, other: Self) -> bool {
		std::mem::discriminant(&self) == std::mem::discriminant(&other)
	}
}

#[cfg(test)]
mod tests {
	use egui_mineral::nav::Route as _;

	use super::*;

	#[test]
	fn every_route_parses_from_its_path() {
		let more = [Route::Workspace, Route::Settings { section: Some(2) }];
		for r in Route::PAGES.into_iter().chain(more) {
			assert_eq!(Route::parse(&r.path()), Ok(r), "{}", r.path());
		}
	}

	#[test]
	fn a_settings_section_is_the_settings_page() {
		assert!(Route::Settings { section: Some(1) }.same_page(Route::Settings { section: None }));
		assert!(!Route::About.same_page(Route::Examples));
	}
}
