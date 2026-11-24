//! Cut, Copy and Paste of project objects (docs/ui/10, Edit; Next's `ObjectClipboard` and `ProjectClipboard`): the
//! objects travel as `.wdproj` fragment text on the system clipboard, the browser's on the web.

use std::cell::RefCell;
use std::rc::Rc;

use waveduderust::io::wdproj::Fragment;
use waveduderust::model::{Project, View};

use crate::tree::Node;

/// Clipboard text read for a paste, for the window's timer to paste (the browser reads asynchronously).
pub(crate) type Pasted = Rc<RefCell<Vec<String>>>;

/// The objects Copy takes from `nodes`: list items or a settings object; `None` when there is none.
pub(crate) fn copy(project: &Project, view: &View, nodes: &[Node]) -> Option<Fragment> {
	let mut f = Fragment::default();
	let mut any = false;
	for &node in nodes {
		any |= match node {
			Node::Scene => f.scene.replace(project.scene.clone()).is_none(),
			Node::Simulation => f.simulation.replace(project.simulation.clone()).is_none(),
			Node::Display => f.view.replace(view.clone()).is_none(),
			Node::Source(i) => project.sources.get(i).map(|s| f.sources.push(s.clone())).is_some(),
			Node::Part(i) => project.parts.get(i).map(|p| f.parts.push(p.clone())).is_some(),
			Node::Probe(i) => project.probes.get(i).map(|p| f.probes.push(p.clone())).is_some(),
			Node::Scheme(i) => project.color_schemes.get(i).map(|s| f.schemes.push(s.clone())).is_some(),
			_ => false,
		};
	}
	any.then_some(f)
}

/// Pastes `fragment` with `current` the current row: a settings object replaces its own on its row or the project
/// root; list items go after the current item of their list, else to its end. The node of the last object pasted.
pub(crate) fn paste(project: &mut Project, view: &mut View, fragment: Fragment, current: Node) -> Option<Node> {
	let mut last = None;
	let accepts = |own: Node| current == own || current == Node::Project;
	if let Some(scene) = fragment.scene.filter(|_| accepts(Node::Scene)) {
		project.scene = scene;
		last = Some(Node::Scene);
	}
	if let Some(simulation) = fragment.simulation.filter(|_| accepts(Node::Simulation)) {
		project.simulation = simulation;
		last = Some(Node::Simulation);
	}
	if let Some(shown) = fragment.view.filter(|_| accepts(Node::Display)) {
		*view = shown;
		last = Some(Node::Display);
	}
	let [sources, parts, probes, schemes] = match current {
		Node::Source(i) => [Some(i), None, None, None],
		Node::Part(i) => [None, Some(i), None, None],
		Node::Probe(i) => [None, None, Some(i), None],
		Node::Scheme(i) => [None, None, None, Some(i)],
		_ => [None; 4],
	};
	last = place(fragment.sources, sources, project.sources.len(), |i, s| {
		project.paste_source(i, s);
	})
	.map(Node::Source)
	.or(last);
	last = place(fragment.parts, parts, project.parts.len(), |i, p| {
		project.paste_part(i, p);
	})
	.map(Node::Part)
	.or(last);
	last = place(fragment.probes, probes, project.probes.len(), |i, p| {
		project.paste_probe(i, p);
	})
	.map(Node::Probe)
	.or(last);
	place(fragment.schemes, schemes, project.color_schemes.len(), |i, s| {
		project.paste_color_scheme(i, s);
	})
	.map(Node::Scheme)
	.or(last)
}

/// Pastes `items` one by one after `current` in a list of `len`, else at its end; the index of the last one.
fn place<T>(items: Vec<T>, current: Option<usize>, len: usize, mut paste: impl FnMut(usize, T)) -> Option<usize> {
	let start = current.map_or(len, |i| (i + 1).min(len));
	let count = items.len();
	for (k, item) in items.into_iter().enumerate() {
		paste(start + k, item);
	}
	(count > 0).then(|| start + count - 1)
}

#[cfg(not(target_arch = "wasm32"))]
thread_local! {
	/// Kept open: on X11 the clipboard's text is served by its owner while it lives, so a copy may not outlast the
	/// app without a clipboard manager.
	static BOARD: RefCell<Option<arboard::Clipboard>> = const { RefCell::new(None) };
}

/// Runs `f` on the system clipboard, opened once.
#[cfg(not(target_arch = "wasm32"))]
fn with_board<T>(f: impl FnOnce(&mut arboard::Clipboard) -> Result<T, arboard::Error>) -> Result<T, String> {
	BOARD.with_borrow_mut(|board| {
		if board.is_none() {
			*board = Some(arboard::Clipboard::new().map_err(|e| e.to_string())?);
		}
		board.as_mut().map_or(Err("no clipboard".into()), |b| f(b).map_err(|e| e.to_string()))
	})
}

/// Puts `text` on the clipboard.
pub(crate) fn write(text: String) -> Result<(), String> {
	#[cfg(not(target_arch = "wasm32"))]
	return with_board(|b| b.set_text(text));
	#[cfg(target_arch = "wasm32")]
	{
		// a Cut does not wait for the answer (divergences.md Clipboard)
		let _ = browser_clipboard().ok_or("no clipboard: the page is not served securely")?.write_text(&text);
		Ok(())
	}
}

/// Reads the clipboard's text into `pasted`: at once, or when the browser answers.
pub(crate) fn read(pasted: &Pasted) {
	#[cfg(not(target_arch = "wasm32"))]
	match with_board(arboard::Clipboard::get_text) {
		Ok(text) => pasted.borrow_mut().push(text),
		Err(e) => tracing::warn!("clipboard: {e}"),
	}
	#[cfg(target_arch = "wasm32")]
	if let Some(clipboard) = browser_clipboard() {
		let (pasted, promise) = (pasted.clone(), clipboard.read_text());
		wasm_bindgen_futures::spawn_local(async move {
			match wasm_bindgen_futures::JsFuture::from(promise).await {
				Ok(text) => pasted.borrow_mut().extend(text.as_string()),
				Err(e) => tracing::warn!("clipboard: {e:?}"),
			}
		});
	}
}

/// The browser's clipboard; absent where the page is not served securely.
#[cfg(target_arch = "wasm32")]
fn browser_clipboard() -> Option<web_sys::Clipboard> {
	use wasm_bindgen::JsCast;
	let navigator = web_sys::window()?.navigator();
	js_sys::Reflect::get(&navigator, &"clipboard".into()).ok().filter(|c| !c.is_undefined()).map(JsCast::unchecked_into)
}

#[cfg(test)]
mod tests {
	use waveduderust::model::Source;

	use super::*;

	fn named(name: &str) -> Source {
		let mut s = Source::default();
		s.name = name.into();
		s
	}

	#[test]
	fn a_source_pastes_after_the_current_one_else_at_the_end() {
		let mut p = Project::default();
		p.sources = vec![named("a"), named("b")];
		let mut v = View::default();
		let clip = copy(&p, &v, &[Node::Source(0)]).unwrap();
		assert_eq!(paste(&mut p, &mut v, clip.clone(), Node::Source(0)), Some(Node::Source(1)));
		assert_eq!(p.sources.iter().map(|s| s.name.as_str()).collect::<Vec<_>>(), ["a", "a copy", "b"]);
		assert_eq!(paste(&mut p, &mut v, clip, Node::Scene), Some(Node::Source(3)));
	}

	#[test]
	fn a_settings_object_replaces_only_on_its_row_or_the_root() {
		let mut p = Project::default();
		let mut v = View::default();
		let mut clip = copy(&p, &v, &[Node::Scene]).unwrap();
		clip.scene.as_mut().unwrap().width = 640;
		assert_eq!(paste(&mut p, &mut v, clip.clone(), Node::Simulation), None);
		assert_eq!(paste(&mut p, &mut v, clip, Node::Project), Some(Node::Scene));
		assert_eq!(p.scene.width, 640);
	}
}
