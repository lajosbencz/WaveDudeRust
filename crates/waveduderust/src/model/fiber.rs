//! A fibre's path editing: the smooth rule that keeps a node's handles opposite, and a new node continuing the path.

use super::{Fiber, FiberNode};

/// Pixels a new node lies on from the last one; its handles are a third of that.
const NEW_NODE_STEP: f64 = 60.0;

fn length(x: f64, y: f64) -> f64 {
	x.hypot(y)
}

fn cell(v: f64) -> i32 {
	v.round_ties_even() as i32
}

/// The unit vector along (x, y); None for a zero vector.
fn direction(x: f64, y: f64) -> Option<(f64, f64)> {
	let l = length(x, y);
	(l > 0.0).then(|| (x / l, y / l))
}

impl FiberNode {
	/// Sets the out handle (`outward`) or the in handle to (dx, dy); a smooth node turns the other handle opposite,
	/// keeping its length.
	pub fn set_handle(&mut self, outward: bool, dx: i32, dy: i32) {
		if outward {
			(self.out_x, self.out_y) = (dx, dy);
		} else {
			(self.in_x, self.in_y) = (dx, dy);
		}
		let l = length(dx.into(), dy.into());
		if !self.smooth || l == 0.0 {
			return;
		}
		let (ox, oy) = if outward { (self.in_x, self.in_y) } else { (self.out_x, self.out_y) };
		let keep = length(ox.into(), oy.into());
		let mirror = (cell(-f64::from(dx) / l * keep), cell(-f64::from(dy) / l * keep));
		if outward {
			(self.in_x, self.in_y) = mirror;
		} else {
			(self.out_x, self.out_y) = mirror;
		}
	}
}

impl Fiber {
	/// Lines node `i`'s handles up through it: without handles along the chord of its neighbours, a third of the
	/// way to each; else the in handle opposite the out handle, or the out handle opposite the in handle.
	pub fn align_node(&mut self, i: usize) {
		let Some(&FiberNode { x, y, in_x, in_y, out_x, out_y, .. }) = self.nodes.get(i) else { return };
		let previous = i.checked_sub(1).and_then(|k| self.nodes.get(k)).map(|n| (f64::from(n.x), f64::from(n.y)));
		let next = self.nodes.get(i + 1).map(|n| (f64::from(n.x), f64::from(n.y)));
		let here = (f64::from(x), f64::from(y));
		let mut out_length = length(out_x.into(), out_y.into());
		let mut in_length = length(in_x.into(), in_y.into());
		let node = &mut self.nodes[i];
		if out_length == 0.0 && in_length == 0.0 {
			if previous.is_none() && next.is_none() {
				return;
			}
			let (from, to) = (previous.unwrap_or(here), next.unwrap_or(here));
			let Some((tx, ty)) = direction(to.0 - from.0, to.1 - from.1) else { return };
			if previous.is_some() {
				let third = length(here.0 - from.0, here.1 - from.1) / 3.0;
				(node.in_x, node.in_y) = (cell(-tx * third), cell(-ty * third));
			}
			if next.is_some() {
				let third = length(to.0 - here.0, to.1 - here.1) / 3.0;
				(node.out_x, node.out_y) = (cell(tx * third), cell(ty * third));
			}
		} else if out_length > 0.0 {
			if in_length == 0.0 && previous.is_some() {
				in_length = out_length;
			}
			node.in_x = cell(-f64::from(out_x) / out_length * in_length);
			node.in_y = cell(-f64::from(out_y) / out_length * in_length);
		} else {
			if next.is_some() {
				out_length = in_length;
			}
			node.out_x = cell(-f64::from(in_x) / in_length * out_length);
			node.out_y = cell(-f64::from(in_y) / in_length * out_length);
		}
	}

	/// Applies the smooth rule to node `i` after an edit from `before`: switched to smooth, its handles line up;
	/// a smooth node's handle edited turns the other opposite.
	pub fn node_edited(&mut self, i: usize, before: &FiberNode) {
		let Some(node) = self.nodes.get_mut(i) else { return };
		if !node.smooth {
			return;
		}
		if !before.smooth {
			self.align_node(i);
		} else if (node.in_x, node.in_y) != (before.in_x, before.in_y) {
			node.set_handle(false, node.in_x, node.in_y);
		} else if (node.out_x, node.out_y) != (before.out_x, before.out_y) {
			node.set_handle(true, node.out_x, node.out_y);
		}
	}

	/// Appends a node continuing the path a step on, along the last node's way out (opposite its in handle, else
	/// from the node before it, else +x), with a handle a third of the step back; the last node gets the matching
	/// out handle when it has none. The first node is at the origin.
	pub fn add_node(&mut self) {
		let Some(last) = self.nodes.last() else {
			self.nodes.push(FiberNode::default());
			return;
		};
		let before = self.nodes.len().checked_sub(2).map(|k| &self.nodes[k]);
		let (dx, dy) = direction(-f64::from(last.in_x), -f64::from(last.in_y))
			.or_else(|| {
				before.and_then(|b| direction(f64::from(last.x) - f64::from(b.x), f64::from(last.y) - f64::from(b.y)))
			})
			.unwrap_or((1.0, 0.0));
		let third = (NEW_NODE_STEP / 3.0).trunc();
		let node = FiberNode {
			x: last.x.saturating_add(cell(dx * NEW_NODE_STEP)),
			y: last.y.saturating_add(cell(dy * NEW_NODE_STEP)),
			in_x: -cell(dx * third),
			in_y: -cell(dy * third),
			..FiberNode::default()
		};
		if let Some(last) = self.nodes.last_mut()
			&& last.out_x == 0
			&& last.out_y == 0
		{
			(last.out_x, last.out_y) = (cell(dx * third), cell(dy * third));
		}
		self.nodes.push(node);
	}
}

#[cfg(test)]
mod tests {
	use super::*;

	fn node(x: i32, y: i32) -> FiberNode {
		FiberNode { x, y, ..FiberNode::default() }
	}

	#[test]
	fn a_smooth_handle_turns_the_other_opposite_keeping_its_length() {
		let mut n = FiberNode { out_x: 0, out_y: 10, ..node(0, 0) };
		n.set_handle(false, 3, 4);
		assert_eq!((n.in_x, n.in_y, n.out_x, n.out_y), (3, 4, -6, -8));
	}

	#[test]
	fn switching_to_smooth_lines_bare_handles_up_along_the_neighbours() {
		let mut fiber = Fiber {
			nodes: vec![node(0, 0), FiberNode { smooth: false, ..node(30, 30) }, node(60, 0)],
			..Fiber::default()
		};
		let before = fiber.nodes[1].clone();
		fiber.nodes[1].smooth = true;
		fiber.node_edited(1, &before);
		let n = &fiber.nodes[1];
		assert_eq!((n.in_x, n.in_y, n.out_x, n.out_y), (-14, 0, 14, 0));
	}

	#[test]
	fn a_new_node_continues_the_path() {
		let mut fiber = Fiber { nodes: vec![node(0, 0)], ..Fiber::default() };
		fiber.add_node();
		assert_eq!(fiber.nodes[1], FiberNode { in_x: -20, ..node(60, 0) });
		assert_eq!((fiber.nodes[0].out_x, fiber.nodes[0].out_y), (20, 0));
		fiber.nodes.clear();
		fiber.add_node();
		assert_eq!(fiber.nodes, [FiberNode::default()]);
	}
}
