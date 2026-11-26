//! Slint list models changed in place, so a repeater keeps the instances of rows that stay.

use slint::{Model, VecModel};

/// Makes `model` hold `rows`: rows that differ are set, missing ones pushed, extra ones removed from the end.
pub(crate) fn update<T: Clone + PartialEq + 'static>(model: &VecModel<T>, rows: impl IntoIterator<Item = T>) {
	let mut len = 0;
	for row in rows {
		match model.row_data(len) {
			Some(old) if old == row => {}
			Some(_) => model.set_row_data(len, row),
			None => model.push(row),
		}
		len += 1;
	}
	while model.row_count() > len {
		model.remove(model.row_count() - 1);
	}
}

#[cfg(test)]
mod tests {
	use super::*;

	fn rows(model: &VecModel<i32>) -> Vec<i32> {
		model.iter().collect()
	}

	#[test]
	fn rows_are_set_grown_and_shrunk_in_place() {
		let model = VecModel::from(vec![1, 2, 3]);
		update(&model, [1, 5, 3]);
		assert_eq!(rows(&model), [1, 5, 3]);
		update(&model, [1, 5, 3, 4, 6]);
		assert_eq!(rows(&model), [1, 5, 3, 4, 6]);
		update(&model, [7]);
		assert_eq!(rows(&model), [7]);
		update(&model, []);
		assert!(rows(&model).is_empty());
	}
}
