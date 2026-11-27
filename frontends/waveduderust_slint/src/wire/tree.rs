//! The tree's rows: selection, check boxes, expanders, drag and drop, keys.

use std::cell::RefCell;
use std::rc::Rc;

use slint::ComponentHandle;

use super::{Handle, dragged, on_node, on_row};
use crate::selection::Pick;
use crate::state::State;
use crate::tree::{self, Node};
use crate::ui::{self, AppWindow};
use crate::{canvas, clipboard, remove};

/// The tree's rows, and a row dropped on the pictures.
pub(crate) fn wire_tree(state: &Rc<RefCell<State>>, app: &AppWindow) {
	app.on_tree_select(on_row(state, app, |s, node, pick| s.selection.pick(&s.doc.project, node, pick)));
	// a row's own check box and buttons act on the group it belongs to, else on the row
	app.on_tree_toggle(on_node(state, app, |s, node| {
		let group = s.selection.group_of(node);
		s.doc.edit(|p| group.iter().for_each(|&n| tree::toggle(p, n)));
	}));
	app.on_tree_expand(on_node(state, app, |s, node| {
		let open = s.collapsed.contains(&node);
		s.expand(node, open);
	}));
	let st = state.clone();
	app.on_tree_drag_data(move |row| {
		let mut data = slint::DataTransfer::default();
		// read lazily, at the drag start, and anew per show(), which replaces the rows; empty while the state is borrowed
		let Ok(s) = st.try_borrow() else { return data };
		if let Some(node) = s.node(row) {
			// the Copy text, for a drop outside the tree
			if let Some(f) = clipboard::copy(&s.doc.project, &s.doc.view, &[node]) {
				data.set_plain_text(waveduderust::io::wdproj::write_fragment(&f).into());
			}
			data.set_user_data(Rc::new(node));
		}
		data
	});
	let st = state.clone();
	app.on_tree_drop_place(move |row, data, before| {
		drop_of(&st.borrow(), row, &data, before).map_or(ui::DropPlace::None, |(_, (place, _))| place)
	});
	let h = Handle::new(state, app);
	app.on_tree_drop(move |row, data, before| {
		h.with(|s, app| {
			let Some((node, (_, to))) = drop_of(s, row, &data, before) else { return };
			let mut moved = node;
			s.doc.edit(|p| moved = tree::move_to(p, node, to));
			s.selection.set(moved);
			s.show(app);
		});
	});
	app.global::<ui::Canvas<'_>>()
		.on_can_drop(|data| matches!(dragged(&data), Some(Node::Source(_) | Node::Part(_) | Node::Probe(_))));
	let h = Handle::new(state, app);
	app.global::<ui::Canvas<'_>>().on_drop(move |data, x, y| {
		h.with(|s, app| {
			let Some(node) = dragged(&data) else { return };
			let Some(fragment) = clipboard::copy(&s.doc.project, &s.doc.view, &[node]) else { return };
			let mut pasted = None;
			s.doc.edit_both(None, |p, v| {
				pasted = clipboard::paste(p, v, fragment, node);
				if let Some(new) = pasted {
					canvas::place(p, &[new], (f64::from(x), f64::from(y)));
				}
			});
			if let Some(new) = pasted {
				s.selection.set(new);
			}
			s.show(app);
		});
	});
	let h = Handle::new(state, app);
	app.on_tree_navigate(move |key, extend| {
		h.with(|s, app| {
			match tree::navigate(&s.doc.project, &s.nodes, &s.collapsed, s.selection.current(), key) {
				Some(tree::Step::To(node)) => {
					s.selection.pick(&s.doc.project, node, if extend { Pick::Range } else { Pick::Alone });
				}
				Some(tree::Step::Expand(node, open)) => s.expand(node, open),
				None => return,
			}
			s.show(app);
		});
	});
	app.on_tree_shift(on_row(state, app, |s, node, by: i32| {
		let mut to = node;
		s.doc.edit(|p| to = tree::shift(p, node, by as isize));
		s.selection.set(to);
	}));
	app.on_tree_remove(on_node(state, app, |s, node| remove(s, &s.selection.group_of(node))));
}

/// The dragged node and where it lands on `row`.
fn drop_of(s: &State, row: i32, data: &slint::DataTransfer, before: bool) -> Option<(Node, (ui::DropPlace, usize))> {
	let (node, target) = dragged(data).zip(s.node(row))?;
	Some((node, tree::drop_target(&s.doc.project, node, target, before)?))
}
