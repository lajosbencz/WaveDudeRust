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

/// A row index from the window, where -1 (any negative) is none.
pub(crate) fn index_from_ui(i: i32) -> Option<usize> {
	usize::try_from(i).ok()
}

/// A row index for the window: -1 for none, or one past `i32::MAX`.
pub(crate) fn index_to_ui(i: Option<usize>) -> i32 {
	i.and_then(|i| i32::try_from(i).ok()).unwrap_or(-1)
}

#[cfg(test)]
mod tests {
	use super::*;

	#[test]
	fn indices_cross_with_minus_one_for_none() {
		assert_eq!(index_from_ui(-1), None);
		assert_eq!(index_from_ui(i32::MIN), None);
		assert_eq!(index_from_ui(0), Some(0));
		assert_eq!(index_from_ui(i32::MAX), Some(i32::MAX as usize));
		assert_eq!(index_to_ui(None), -1);
		assert_eq!(index_to_ui(Some(3)), 3);
		assert_eq!(index_to_ui(Some(usize::MAX)), -1);
		for i in [-1, 0, 7] {
			assert_eq!(index_to_ui(index_from_ui(i)), i);
		}
	}

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
